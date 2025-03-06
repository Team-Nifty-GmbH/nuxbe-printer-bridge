use std::sync::{Arc, Mutex};

use actix_web::{get, post, web, Responder, HttpResponse};

use crate::models::{Config, ConfigUpdateRequest};
use crate::config::save_config;

/// GET /config - Get current configuration
#[get("/config")]
pub async fn get_config(config: web::Data<Arc<Mutex<Config>>>) -> impl Responder {
    let config = config.lock().unwrap().clone();
    HttpResponse::Ok().json(config)
}

/// POST /config - Update configuration
#[post("/config")]
pub async fn update_config(
    config_data: web::Data<Arc<Mutex<Config>>>,
    new_config: web::Json<ConfigUpdateRequest>,
) -> impl Responder {
    let mut config = config_data.lock().unwrap();
    *config = new_config.config.clone();
    save_config(&config);
    HttpResponse::Ok().json(config.clone())
}