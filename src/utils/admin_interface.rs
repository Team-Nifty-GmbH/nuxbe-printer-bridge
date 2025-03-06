use std::fs;
use crate::config::admin_dir;

/// Create the admin interface files
pub fn create_admin_interface() -> std::io::Result<()> {
    // Create admin directory if it doesn't exist
    let admin_dir = admin_dir();
    fs::create_dir_all(&admin_dir)?;

    // Create index.html with admin interface
    let html = include_str!("admin_interface.html");
    fs::write(admin_dir.join("index.html"), html)?;

    Ok(())
}

// We'll keep the HTML content in a separate file for better maintainability
// Create a file at src/utils/admin_interface.html with the HTML content