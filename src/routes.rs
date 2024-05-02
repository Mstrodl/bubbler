use actix_web::http::StatusCode;
use actix_web::{get, post, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

pub mod config;
pub mod machine;
use config::AppData;
use machine::DropError;

#[derive(Serialize, Deserialize)]
struct HealthReport {
    slots: Vec<String>,
    temp: f32,
}
#[derive(Serialize)]
struct SlotReport {
    slots: Vec<machine::SlotStatus>,
    temp: f32,
}

#[derive(Serialize, Deserialize)]
struct DropRequest {
    slot: usize,
}

#[derive(Serialize)]
struct DropResponse {
    message: String,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct DropErrorRes {
    error: String,
    errorCode: u16,
}

#[post("/drop")]
async fn drop(data: web::Data<AppData>, req_body: web::Json<DropRequest>) -> impl Responder {
    let drop_result = {
        let config = data.config.lock().await;
        machine::drop(config.deref(), req_body.slot).await
    };
    match drop_result {
        Ok(_) => HttpResponse::Ok().json(DropResponse {
            message: "Dropped drink from slot ".to_string() + &req_body.slot.to_string(),
        }),
        Err(DropError::BadSlot) => {
            HttpResponse::Ok()
                .status(StatusCode::BAD_REQUEST)
                .json(DropErrorRes {
                    error: "Invalid slot ID provided".to_string(),
                    errorCode: 400,
                })
        }
        Err(err) => HttpResponse::Ok()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .json(DropErrorRes {
                error: err.to_string(),
                errorCode: 500,
            }),
    }
}

#[get("/health")]
async fn health(data: web::Data<AppData>) -> impl Responder {
    let config = data.config.lock().await;
    let slots = machine::get_slots_old(config.deref());
    let temperature = machine::get_temperature(config.deref());

    let temperature = temperature * (9.0 / 5.0) + 32.0;

    HttpResponse::Ok().json(HealthReport {
        slots: slots.to_vec(),
        temp: temperature,
    })
}

#[get("/slots")]
async fn get_slots(data: web::Data<AppData>) -> impl Responder {
    let config = data.config.lock().await;
    let slots = machine::get_slots(config.deref());
    let temp = machine::get_temperature(config.deref());

    HttpResponse::Ok().json(SlotReport { slots, temp })
}
