
use bevy::{
    core::FrameCount,
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::{CursorGrabMode, PresentMode, WindowLevel, WindowTheme},
};

use bevy_simple_text_input::{
    TextInputBundle, TextInputInactive, TextInputPlugin, TextInputSystem, TextInputSubmitEvent
};

use bevy_defer::AsyncCommandsExtension;

const BORDER_COLOR_ACTIVE: Color = Color::srgb(0.75, 0.52, 0.99);
const BORDER_COLOR_INACTIVE: Color = Color::srgb(0.25, 0.25, 0.25);
const TEXT_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const BACKGROUND_COLOR: Color = Color::srgb(0.15, 0.15, 0.15);


pub async fn open_gui_window(cli_args: &crate::cli::Args) -> Result<(), Box<dyn std::error::Error>> {
  App::new()
    .add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Oliana".into(),
                name: Some("Oliana".into()),
                resolution: (500., 300.).into(),
                present_mode: PresentMode::AutoVsync,
                window_theme: Some(WindowTheme::Dark),
                enabled_buttons: bevy::window::EnabledButtons {
                    maximize: false,
                    ..Default::default()
                },
                // This will spawn an invisible window
                // The window will be made visible in the make_visible() system after 3 frames.
                // This is useful when you want to avoid the white window that shows up before the GPU is ready to render the app.
                visible: false,
                ..default()
            }),
            ..default()
        }),
        LogDiagnosticsPlugin::default(),
        FrameTimeDiagnosticsPlugin,
    ))
    .add_plugins(bevy_defer::AsyncPlugin::default_settings())
    .add_plugins(TextInputPlugin)

    .add_event::<OllamaIsReadyToProcessEvent>()
    .add_event::<PromptToOllamaEvent>()
    .add_event::<ResponseFromOllamaEvent>()

    .insert_resource((*cli_args).clone()) // Accept a Ref<crate::cli::Args> in your system's function to read cli args in the UI
    .insert_resource(OllamaResource::default()) // Accept a Ref<crate::gui::OllamaResource> in your system's function to touch the Ollama stuff

    .add_systems(
        Update,
        (
            make_visible,
        ),
    )
    .add_systems(Startup, (setup, setup_ollama) )
    .add_systems(Update, focus.before(TextInputSystem))
    .add_systems(Update, text_listener.after(TextInputSystem))
    .add_systems(Update, read_ollama_ready_events)
    .add_systems(Update, read_ollama_response_events)
    .add_systems(Update, read_ollama_prompt_events)

   .run();

   Ok(())
}

fn make_visible(mut window: Query<&mut Window>, frames: Res<FrameCount>) {
    // The delay may be different for your app or system.
    if frames.0 == 3 {
        // At this point the gpu is ready to show the app so we can make the window visible.
        // Alternatively, you could toggle the visibility in Startup.
        // It will work, but it will have one white frame before it starts rendering
        window.single_mut().visible = true;
    }
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());

    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::End, // End here means "Bottom"
                    justify_content: JustifyContent::Start, // Start here means "Left"
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                ..default()
            },
            // Make this container node bundle to be Interactive so that clicking on it removes
            // focus from the text input.
            Interaction::None,
        ))
        .with_children(|parent| {
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(80.0),
                        border: UiRect::all(Val::Px(2.0)),
                        padding: UiRect::all(Val::Px(4.0)),
                        ..default()
                    },
                    border_color: BORDER_COLOR_INACTIVE.into(),
                    background_color: BACKGROUND_COLOR.into(),
                    // Prevent clicks on the input from also bubbling down to the container
                    // behind it
                    focus_policy: bevy::ui::FocusPolicy::Block,
                    ..default()
                },
                TextInputBundle::default()
                    .with_text_style(TextStyle {
                        font_size: 32.0,
                        color: TEXT_COLOR,
                        ..default()
                    })
                    //.with_placeholder("Click to Type Text", None)
                    .with_inactive(true),
            ));
        });

    // Text with one section; OllamaReplyText allows us to refer to the TextBundle?
    commands.spawn((
        // Create a TextBundle that has a Text with a single section.
        TextBundle::from_section(
            // Accepts a `String` or any type that converts into a `String`, such as `&str`
            "hello\nbevy!",
            TextStyle {
                // This font is loaded and will be used instead of the default font.
                // font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                font_size: 32.0,
                ..default()
            },
        ) // Set the justification of the Text
        .with_text_justify(JustifyText::Center)
        // Set the style of the TextBundle itself.
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(4.0),
            left: Val::Px(4.0),
            ..default()
        }),
        OllamaReplyText,
    ));

}

fn focus(
    query: Query<(Entity, &Interaction), Changed<Interaction>>,
    mut text_input_query: Query<(Entity, &mut TextInputInactive, &mut BorderColor)>,
) {
    for (interaction_entity, interaction) in &query {
        if *interaction == Interaction::Pressed {
            for (entity, mut inactive, mut border_color) in &mut text_input_query {
                if entity == interaction_entity {
                    inactive.0 = false;
                    *border_color = BORDER_COLOR_ACTIVE.into();
                } else {
                    inactive.0 = true;
                    *border_color = BORDER_COLOR_INACTIVE.into();
                }
            }
        }
    }
}

fn text_listener(mut events: EventReader<TextInputSubmitEvent>, mut event_writer: EventWriter<PromptToOllamaEvent>,) {
    for event in events.read() {
        info!("{:?} submitted: {}", event.entity, event.value);
        event_writer.send(PromptToOllamaEvent(event.value.clone()));
    }
}

fn read_ollama_ready_events(
    mut event_reader: EventReader<OllamaIsReadyToProcessEvent>,
) {
    for ev in event_reader.read() {
        eprintln!("Event {:?} recieved!", ev);
    }
}

fn read_ollama_response_events(
    mut event_reader: EventReader<ResponseFromOllamaEvent>,
    mut query: Query<&mut Text, With<OllamaReplyText>>
) {
    for ev in event_reader.read() {
        eprintln!("Event {:?} recieved!", ev);
        for mut text in &mut query { // We'll only ever have 1 section of text rendered
            text.sections[0].value = ev.0.to_string();
        }
    }
}

fn read_ollama_prompt_events(
    mut commands: Commands,
    mut event_reader: EventReader<PromptToOllamaEvent>,
    // mut event_writer: EventWriter<ResponseFromOllamaEvent>,
    mut ollama_resource: ResMut<crate::gui::OllamaResource>,
) {
    let arc_to_ollama_rwlock = ollama_resource.into_inner().ollama_inst.clone();
    for ev in event_reader.read() {
        let ev_txt = ev.0.to_string();
        eprintln!("Passing this prompt to Ollama: {:?}", ev.0);

        let closure_arc_to_ollama_rwlock = arc_to_ollama_rwlock.clone();

        commands.spawn_task(|| async move {
            let ollama_resource_readlock = std::sync::RwLock::read(&closure_arc_to_ollama_rwlock).expect("Could not get read-only access to Ollama instance!");

            let ollama_resp = ollama_resource_readlock.generate(ollama_rs::generation::completion::request::GenerationRequest::new("qwen2.5:7b".to_string(), ev_txt)).await;

            match ollama_resp {
                Ok(resp) => {
                    eprintln!("resp.response = {:?}", resp.response);
                    bevy_defer::access::AsyncWorld.send_event(ResponseFromOllamaEvent(resp.response.to_string())).expect("async error");
                }
                Err(e) => {
                    eprintln!("resp e = {:?}", e);
                    bevy_defer::access::AsyncWorld.send_event(ResponseFromOllamaEvent(format!("{:?}", e))).expect("async error");
                }
            }


            Ok(())
        });/*
        .add(move |w: &mut World| {
            // Poll for a reply up to 90 seconds; forwarding first non-empty value we unpack
            //let resp_str = sync_boxed_reply.read().expect("Cannot aquire read lock!").to_string();
            //w.send_event(ResponseFromOllamaEvent(resp_str));
            let mut remaining_polls: usize = 90 * 4;
            loop {
                if remaining_polls < 1 {
                    eprintln!("TIMED OUT Waiting for Ollama reply!");
                    break;
                }
                remaining_polls -= 1;

                match sync_boxed_reply.try_read() {
                    Ok(resp_str) => {
                        if resp_str.len() > 0 {
                            w.send_event(ResponseFromOllamaEvent(resp_str.to_string()));
                            break;
                        }
                        else {
                            // Read success, but no data rx-ed yet so we fall through
                        }
                    }
                    Err(e) => {
                        // Fall-through
                    }
                };

                // No data rx-ed yet OR error recieving so we wait
                std::thread::sleep(std::time::Duration::from_millis(250));
            }

        });*/

    }
}


fn setup_ollama(mut commands: Commands, mut ollama_resource: ResMut<crate::gui::OllamaResource>, cli_args: Res<crate::cli::Args>, mut ev_ollama_ready: EventWriter<OllamaIsReadyToProcessEvent>) {

    // The write lock is NOT dropped here, it is MOVED into the async context below.
    let owned_cli_args: crate::cli::Args = cli_args.clone();
    let arc_to_ollama_rwlock = ollama_resource.into_inner().ollama_inst.clone();

    commands.spawn_task(|| async move {

        let mut ollama_resource_writelock = std::sync::RwLock::write(&arc_to_ollama_rwlock).expect("Cannot get Write lock of OllamaResource.ollama_inst");
        *ollama_resource_writelock = crate::ai::init_ollama_with_model_pulled(&owned_cli_args, "qwen2.5:7b").await.unwrap();

        // std::thread::sleep(std::time::Duration::from_millis(3500)); // Haha yeah we suck, but this is a good knee-jerk measurement of ^^ lotsa work upstairs

        Ok(())
    })
    .add(|w: &mut World| {
        w.send_event(OllamaIsReadyToProcessEvent());
    });
}



#[derive(Debug, Clone, Default, bevy::ecs::system::Resource)]
pub struct OllamaResource {
    pub ollama_inst: std::sync::Arc<std::sync::RwLock<ollama_rs::Ollama>>,
}

#[derive(Debug, bevy::ecs::event::Event)]
pub struct OllamaIsReadyToProcessEvent();

#[derive(Debug, bevy::ecs::event::Event)]
pub struct PromptToOllamaEvent(String);

#[derive(Debug, bevy::ecs::event::Event)]
pub struct ResponseFromOllamaEvent(String);


// A unit struct to help identify the Ollama Reply UI component, since there may be many Text components
#[derive(Component)]
struct OllamaReplyText;
