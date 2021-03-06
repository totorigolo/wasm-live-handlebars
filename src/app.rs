use crate::{
    agents::{NotificationBus, NotificationSender},
    components::{Navbar, Notifications},
    prelude::*,
    scenario::Scenario,
    template_engine::{HandlebarsEngine, TemplateEngine},
    InputsData, Path,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use yew::{
    agent::{Dispatched, Dispatcher},
    format::Json as YewJson,
    services::storage::{Area, StorageService},
    Component, ComponentLink, Html, ShouldRender,
};

use crate::inputs::*;

lazy_static! {
    static ref LOCAL_STORAGE_KEY: String =
        { format!("totorigolo.{}.state", env!("CARGO_PKG_NAME")) };
}

const JSON_INPUT: &str = include_str!("input_data.json");
const INPUT_TEMPLATE: &str = include_str!("input_template.hbs");

pub struct App {
    link: ComponentLink<Self>,
    template_engine: HandlebarsEngine,
    storage: StorageService,
    notification_bus: Dispatcher<NotificationBus>,
    state: State,
    on_navevent: Callback<NavEvent>,
}

#[derive(Serialize, Deserialize, Debug)]
enum State {
    Init,
    Loaded {
        scenario: Scenario,
        #[serde(default)]
        inputs_data: InputsData,
    },
}

#[derive(Debug)]
pub enum Msg {
    Init,
    NavEvent(NavEvent),
    FetchedJsonData(String),
    SaveToLocalStorage,
    EditedInput(Path, JsonValue),
    ListInputSizeChanged(Path, usize),
    RemoveAt(Path),
}

#[derive(Debug)]
pub enum NavEvent {
    LoadDebugScenario,
    LoadFromLocalStorage,
    UnloadScenario,
}

impl NotificationSender for App {
    fn notification_bus(&mut self) -> &mut Dispatcher<NotificationBus> {
        &mut self.notification_bus
    }
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Msg::Init);
        let on_navevent = link.callback(Msg::NavEvent);

        Self {
            link,
            template_engine: HandlebarsEngine::new_uninit(),
            storage: StorageService::new(Area::Local).expect("Failed to get localStorage."),
            notification_bus: NotificationBus::dispatcher(),
            state: State::Init,
            on_navevent,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        trace!("Received: {:?}", msg);
        match msg {
            Msg::Init => {
                self.state = State::Init;
                true
            }
            Msg::NavEvent(nav_event) => {
                match nav_event {
                    NavEvent::LoadDebugScenario => self.load_debug_scenario(),
                    NavEvent::LoadFromLocalStorage => self.load_from_local_storage(),
                    NavEvent::UnloadScenario => self.unload_scenario(),
                }
            }
            Msg::FetchedJsonData(json_str) => match self.load_from_json(&json_str) {
                Ok(should_render) => should_render,
                Err(e) => {
                    // TODO: Better log when the log will be an enum --v
                    let error = e.context("Failed to load the received scenario.");
                    self.notif_error(format!("{:?}", error));
                    false
                }
            },
            Msg::SaveToLocalStorage => {
                self.storage
                    .store(LOCAL_STORAGE_KEY.as_ref(), YewJson(&self.state));
                false
            }
            Msg::EditedInput(path, value) => match &mut self.state {
                State::Loaded { inputs_data, .. } => {
                    match inputs_data.insert_at(&path, value) {
                        Ok(()) => self.link.send_message(Msg::SaveToLocalStorage),
                        Err(e) => {
                            // TODO: Show the error
                            error!("Failed to save value of '{}': {:?}", path, e);
                        }
                    }
                    true
                }
                _ => {
                    warn!(
                        "Shouldn't have received a Msg::EditedInput message in state: {:?}.",
                        self.state
                    );
                    false
                }
            },
            Msg::ListInputSizeChanged(path, new_size) => match &mut self.state {
                State::Loaded { inputs_data, .. } => {
                    if let Err(e) = inputs_data.resize_array_at(&path, new_size) {
                        warn!("Failed to access array at '{}': {:?}", path, e);
                    }

                    self.link.send_message(Msg::SaveToLocalStorage);
                    true
                }
                _ => {
                    warn!(
                        "Shouldn't have received a Msg::ListInputSizeChanged message in state: \
                         {:?}.",
                        self.state
                    );
                    false
                }
            },
            Msg::RemoveAt(path) => match &mut self.state {
                State::Loaded { inputs_data, .. } => {
                    if let Err(e) = inputs_data.remove_at(&path) {
                        warn!("Failed to remove at '{}': {:?}", path, e);
                    }

                    self.link.send_message(Msg::SaveToLocalStorage);
                    true
                }
                _ => {
                    warn!(
                        "Shouldn't have received a Msg::RemoveAt message in state: {:?}.",
                        self.state
                    );
                    false
                }
            },
        }
    }

    fn view(&self) -> Html {
        let state_html = match &self.state {
            State::Init => {
                html! {
                    <div class="box">
                        <p>{ "Nothing loaded. Use a button above." }</p>
                    </div>
                }
            }
            State::Loaded {
                scenario,
                inputs_data,
                ..
            } => {
                html! {
                    <div class="columns is-desktop">
                        <div class="column">
                            { render_inputs(&scenario.inputs, inputs_data, &self.link) }
                        </div>
                        <div class="column">
                            { render_code_column(inputs_data, &self.template_engine) }
                        </div>
                    </div>
                }
            }
        };

        html! {
            <>
                <Notifications />

                <div class="section">
                    <div class="container navbar-container">
                        <div class="box">
                            <Navbar on_navevent=&self.on_navevent />
                        </div>
                    </div>
                </div>

                <div class="section site-content">
                    <div class="container">
                        { state_html }
                    </div>
                </div>

                <footer class="footer">
                    <div class="content has-text-centered">
                        <p>{ "Wonderful footer" }</p>
                    </div>
                </footer>
            </>
        }
    }
}

impl App {
    fn load_from_json(&mut self, json_str: &str) -> Result<ShouldRender> {
        let mut json_data: JsonValue = serde_json::from_str(&json_str).context("Invalid JSON.")?;
        let template = serde_json::from_value(json_data["template"].take())
            .context("JSON input must have a template.")?;

        let inputs = serde_json::from_value(json_data["inputs"].take())
            .context("Failed to deserialize inputs")?;

        self.template_engine
            .set_template(&template)
            .map_err(|e| e.context("Failed to load the template"))?;

        self.state = State::Loaded {
            scenario: Scenario { template, inputs },
            inputs_data: InputsData::default(),
        };
        self.link.send_message(Msg::SaveToLocalStorage);

        Ok(true)
    }

    fn load_debug_scenario(&mut self) -> ShouldRender {
        let json_str = JSON_INPUT.replace("%TEMPLATE%", &INPUT_TEMPLATE.replace("\n", "\\n"));
        self.link.send_message(Msg::FetchedJsonData(json_str));
        false
    }

    fn load_from_local_storage(&mut self) -> ShouldRender {
        if let YewJson(Ok(restored_state)) = self.storage.restore(LOCAL_STORAGE_KEY.as_ref()) {
            self.state = restored_state;

            // Initialize the template engine with the deserialized template.
            // This can fail if the restored state is somewhat invalid.
            if let State::Loaded { scenario, .. } = &self.state {
                if let Err(e) = self.template_engine.set_template(&scenario.template) {
                    self.storage.remove(LOCAL_STORAGE_KEY.as_ref());
                    self.state = State::Init;
                    self.link.send_message(Msg::Init);

                    self.notif_error(format!(
                        "Invalid template fetched from local storage: {}",
                        e
                    ));
                }
            }

            if let State::Init = self.state {
                // No notification
            } else {
                self.notif_success("Restored previous session.");
            }

            true
        } else {
            // If we're here, local storage is either absent or invalid
            self.notif_warn("Nothing to restore from local storage.");
            self.storage.remove(LOCAL_STORAGE_KEY.as_ref());
            self.link.send_message(Msg::Init);
            false
        }
    }

    fn unload_scenario(&mut self) -> ShouldRender {
        self.link.send_message(Msg::Init);
        false
    }
}

fn render_inputs(
    inputs: &[InputTypes],
    inputs_data: &InputsData,
    link: &ComponentLink<App>,
) -> Html {
    use crate::views::RenderableInput;

    html! {
        <div class="box">
            <h1 class="title">{ "Inputs" }</h1>
            { for inputs.iter().map(|input| input.render(&Path::default(), inputs_data, link)) }
        </div>
    }
}

fn render_code_column<T: TemplateEngine>(inputs_data: &InputsData, template_engine: &T) -> Html {
    let rendered = template_engine
        .render(inputs_data)
        .unwrap_or_else(|e| e.context("Failed to render the data").to_string());

    html! {
        <>
            <div class="box">
                <h1 class="title">{ "Rendered template" }</h1>
                <pre>{rendered}</pre>
            </div>
            <div class="box">
                <h1 class="title">{ "Data" }</h1>
                <pre>{ format!("{:#}", inputs_data) }</pre>
            </div>
        </>
    }
}
