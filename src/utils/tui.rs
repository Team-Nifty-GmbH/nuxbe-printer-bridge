use cursive::align::HAlign;
use cursive::traits::*;
use cursive::view::Margins;
use cursive::views::{Checkbox, Dialog, EditView, LinearLayout, PaddedView, TextView};
use cursive::Cursive;
use std::sync::{Arc, Mutex};

use crate::config::{load_config, save_config};
use crate::models::Config;

/// Start the TUI editor for application settings
pub fn run_tui() {
    // Load the current configuration
    let config = Arc::new(Mutex::new(load_config()));

    // Create the Cursive root
    let mut siv = cursive::default();

    // Create main configuration form
    create_config_dialog(&mut siv, config);

    // Run the event loop
    siv.run();
}

/// Create the main configuration dialog
fn create_config_dialog(siv: &mut Cursive, config: Arc<Mutex<Config>>) {
    // Clone the config to use in the UI
    let current_config = config.lock().unwrap().clone();

    let server_settings = create_server_settings(&current_config);
    let interval_settings = create_interval_settings(&current_config);
    let api_settings = create_api_settings(&current_config);
    let reverb_settings = create_reverb_settings(&current_config);

    // Create the main dialog
    siv.add_layer(
        Dialog::new()
            .title("FLUX <-> CUPS Print Server Configuration")
            .content(
                LinearLayout::vertical()
                    .child(server_settings)
                    .child(interval_settings)
                    .child(api_settings)
                    .child(reverb_settings)
                    .child(
                        TextView::new(
                            "Changes will be applied after saving and restarting the server.",
                        )
                        .h_align(HAlign::Center),
                    ),
            )
            .button("Save", move |s| {
                save_config_from_ui(s, Arc::clone(&config));
            })
            .button("Cancel", |s| {
                s.quit();
            }),
    );
}

/// Create server settings section
fn create_server_settings(config: &Config) -> impl View {
    PaddedView::new(
        Margins::lrtb(1, 1, 0, 1),
        Dialog::around(
            LinearLayout::vertical()
                .child(TextView::new("Instance Name:"))
                .child(
                    EditView::new()
                        .content(config.instance_name.clone())
                        .with_name("instance_name"),
                )
                .child(TextView::new("API Port:"))
                .child(
                    EditView::new()
                        .content(config.api_port.to_string())
                        .with_name("api_port"),
                ),
        )
        .title("Server Settings"),
    )
}

/// Create interval settings section
fn create_interval_settings(config: &Config) -> impl View {
    PaddedView::new(
        Margins::lrtb(1, 1, 0, 1),
        Dialog::around(
            LinearLayout::vertical()
                .child(TextView::new("Printer Check Interval (minutes):"))
                .child(
                    EditView::new()
                        .content(config.printer_check_interval.to_string())
                        .with_name("printer_check_interval"),
                )
                .child(TextView::new("Job Check Interval (minutes):"))
                .child(
                    EditView::new()
                        .content(config.job_check_interval.to_string())
                        .with_name("job_check_interval"),
                ),
        )
        .title("Polling Intervals"),
    )
}

/// Create API settings section
fn create_api_settings(config: &Config) -> impl View {
    PaddedView::new(
        Margins::lrtb(1, 1, 0, 1),
        Dialog::around(
            LinearLayout::vertical()
                .child(TextView::new("Flux Url:"))
                .child(
                    EditView::new()
                        .content(config.flux_url.clone())
                        .with_name("flux_url"),
                )
             
                .child(TextView::new("Flux Api Token:"))
                .child(
                    EditView::new()
                        .content(config.flux_api_token.clone().unwrap())
                        .with_name("flux_api_token"),
                ),
        )
        .title("API Integration"),
    )
}

/// Create WebSocket settings section
fn create_reverb_settings(config: &Config) -> impl View {
    let reverb_disabled = Checkbox::new()
        .with_checked(config.reverb_disabled)
        .with_name("reverb_disabled");
    
    // Create a layout for the Reverb settings
    let mut layout = LinearLayout::vertical()
        .child(LinearLayout::horizontal()
                   .child(reverb_disabled)
                   .child(TextView::new("Disable Websockets: ")),)
        .child(TextView::new("Reverb App ID:"))
        .child(
            EditView::new()
                .content(config.reverb_app_id.clone())
                .with_name("reverb_app_id"),
        )
        .child(TextView::new("Reverb App Key:"))
        .child(
            EditView::new()
                .content(config.reverb_app_key.clone())
                .with_name("reverb_app_key"),
        )
        .child(TextView::new("Reverb App Secret:"))
        .child(
            EditView::new()
                .content(config.reverb_app_secret.clone())
                .with_name("reverb_app_secret"),
        );

    // Add the TLS checkbox
    let use_tls = Checkbox::new()
        .with_checked(config.reverb_use_tls)
        .with_name("reverb_use_tls");

    layout.add_child(
        LinearLayout::horizontal()
            .child(use_tls)
            .child(TextView::new(" Use TLS for Reverb Connection")),
    );

    // Add the host field
    layout.add_child(TextView::new("Reverb Host"));
    layout.add_child(
        EditView::new()
            .content(config.reverb_host.clone().unwrap_or_default())
            .with_name("reverb_host"),
    );

    // Add the host field
    layout.add_child(TextView::new("Reverb Auth Endpoint"));
    layout.add_child(
        EditView::new()
            .content(config.reverb_auth_endpoint.clone())
            .with_name("reverb_auth_endpoint"),
    );

    PaddedView::new(
        Margins::lrtb(1, 1, 0, 1),
        Dialog::around(layout).title("Laravel Reverb WebSocket Settings"),
    )
}

/// Save configuration from UI values
fn save_config_from_ui(s: &mut Cursive, config: Arc<Mutex<Config>>) {
    // Get a mutable reference to the config
    let mut config_guard = config.lock().unwrap();

    // Update server settings
    config_guard.instance_name = s
        .call_on_name("instance_name", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.api_port = s
        .call_on_name("api_port", |view: &mut EditView| {
            view.get_content().parse::<u16>().unwrap_or(8080)
        })
        .unwrap_or(8080);

    // Update interval settings
    config_guard.printer_check_interval = s
        .call_on_name("printer_check_interval", |view: &mut EditView| {
            view.get_content().parse::<u64>().unwrap_or(5)
        })
        .unwrap_or(5);

    config_guard.job_check_interval = s
        .call_on_name("job_check_interval", |view: &mut EditView| {
            view.get_content().parse::<u64>().unwrap_or(2)
        })
        .unwrap_or(2);

    // Update API settings
    config_guard.flux_url = s
        .call_on_name("flux_url", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.flux_api_token = Option::from(s
        .call_on_name("flux_api_token", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default());

    config_guard.reverb_disabled = s
        .call_on_name("reverb_disabled", |view: &mut Checkbox| view.is_checked())
        .unwrap_or(false);
    
    // Update WebSocket settings
    config_guard.reverb_app_id = s
        .call_on_name("reverb_app_id", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.reverb_app_key = s
        .call_on_name("reverb_app_key", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.reverb_app_secret = s
        .call_on_name("reverb_app_secret", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.reverb_use_tls = s
        .call_on_name("reverb_use_tls", |view: &mut Checkbox| view.is_checked())
        .unwrap_or(true);

    let reverb_host = s
        .call_on_name("reverb_host", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    config_guard.reverb_host = if reverb_host.is_empty() {
        None
    } else {
        Some(reverb_host)
    };

    config_guard.reverb_auth_endpoint = s
        .call_on_name("reverb_auth_endpoint", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    // Save the updated configuration
    save_config(&config_guard);

    // Show success dialog
    s.add_layer(
        Dialog::around(TextView::new("Configuration saved successfully!"))
            .title("Success")
            .button("OK", |s| {
                s.pop_layer();
                s.quit();
            }),
    );
}
