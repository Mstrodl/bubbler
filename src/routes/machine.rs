use crate::scheduler::RealtimeGuard;
use futures::stream::StreamExt;
use gpio_cdev::{EventRequestFlags, Line, LineRequestFlags};
use serde::Serialize;

use super::config::{ConfigData, SlotConfig, SlotConfig::*};
use std::fmt::{self, Debug, Display, Formatter};
use std::fs;
use std::thread;
use std::time::Duration;

pub fn get_temperature(config: &ConfigData) -> f32 {
    let temperature_id = &config.temperature_id;
    if temperature_id.is_empty() {
        return 0.0;
    }
    let path = format!("/mnt/w1/{}/temperature12", temperature_id);
    let temperature = fs::read_to_string(path.clone());

    match temperature {
        Ok(temperature) => match temperature.trim_end().parse::<f32>() {
            Ok(temperature) => temperature,
            Err(err) => {
                eprintln!("Temperature sensor {} errored out: {:?}", path, err);
                0.0
            }
        },
        Err(_) => {
            eprintln!("Temperature sensor {} doesn't exist!", path);
            0.0
        }
    }
}

fn is_stocked(slot: &SlotConfig) -> bool {
    match slot {
        GPIO { stocked, .. } => stocked.get_value().unwrap() == 1,
        OWFS(id) => fs::File::open(format!("/mnt/w1/{}/id", id)).is_ok(),
    }
}

// TODO: Why the heck is the API like this?
pub fn get_slots_old(config: &ConfigData) -> Vec<String> {
    let mut slots: Vec<String> = Vec::new();
    for slot in &config.slots {
        slots.push(match is_stocked(slot) {
            false => format!("Slot {} ({}) is empty", slots.len() + 1, slot),
            true => format!("Slot {} ({}) is stocked", slots.len() + 1, slot),
        })
    }
    slots
}

#[derive(Serialize)]
pub struct SlotStatus {
    pub id: String,
    pub number: i32,
    pub stocked: bool,
}
pub fn get_slots(config: &ConfigData) -> Vec<SlotStatus> {
    config
        .slots
        .iter()
        .enumerate()
        .map(|(number, slot)| SlotStatus {
            id: format!("{}", slot),
            number: number as i32,
            stocked: is_stocked(slot),
        })
        .collect()
}

#[derive(Debug)]
pub enum DropState {
    Success,
}

impl Display for DropError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MotorFailed => write!(f, "Motor didn't actuate"),
            Self::MotorTimeout => write!(f, "Motor timed out. Is it stuck?"),
            Self::BadSlot => write!(f, "Bad slot ID"),
        }
    }
}

#[derive(Debug)]
pub enum DropError {
    MotorFailed,
    MotorTimeout,
    BadSlot,
}

pub fn run_motor(slot: &SlotConfig, state: bool) -> Result<DropState, DropError> {
    let num_state = match state {
        true => 1,
        false => 0,
    };
    let motor_okay = match slot {
        OWFS(slot_id) => fs::write(format!("/mnt/w1/{}/PIO", slot_id), num_state.to_string())
            .map_err(|err| format!("{:?}", err)),
        GPIO { vend, .. } => vend
            .set_value(num_state)
            .map_err(|err| format!("{:?}", err)),
    };
    match motor_okay {
        Err(err) => {
            println!("Error actuating motor: {}", err);
            Err(DropError::MotorFailed)
        }
        Ok(_) => Ok(DropState::Success),
    }
}

async fn wait_until_line_hits_value(
    line: &Line,
    edge: EventRequestFlags,
    timeout: Duration,
) -> Result<(), DropError> {
    let mut event_handle = line
        .async_events(LineRequestFlags::INPUT, edge, "bub-cam-events")
        .unwrap();
    tokio::time::timeout(timeout, event_handle.next())
        .await
        .map_err(|_| DropError::MotorTimeout)?;
    Ok(())
}

pub async fn drop(config: &ConfigData, slot: usize) -> Result<DropState, DropError> {
    if slot > config.slots.len() || slot == 0 {
        eprintln!("We were asked to drop an invalid slot {}: BadSlot!", slot);
        return Err(DropError::BadSlot);
    }

    let slot_config = &config.slots[slot - 1];
    println!("Dropping {}!", slot_config);

    let mut result = Ok(DropState::Success);
    if let Some(latch) = config.latch.as_ref() {
        latch.open();
    }
    let _rt = RealtimeGuard::default();
    if let Err(err) = run_motor(slot_config, true) {
        eprintln!("Problem dropping {} ({})! {:?}", slot, slot_config, err);
        result = Err(err);
    } else if let SlotConfig::GPIO { cam: Some(cam), .. } = slot_config {
        println!("Waiting for motor to start rotating...",);
        if let Err(err) = wait_until_line_hits_value(
            cam,
            EventRequestFlags::RISING_EDGE,
            Duration::from_millis(500),
        )
        .await
        {
            eprintln!("Were we already been spinning? {err:?}");
        }
        println!("Waiting for motor to stop rotating...");
        if let Err(err) = wait_until_line_hits_value(
            cam,
            EventRequestFlags::FALLING_EDGE,
            Duration::from_secs(10),
        )
        .await
        {
            result = Err(err);
        }
        println!("Motor stopped rotating!",);
    } else {
        println!("Sleeping for {}ms after dropping", config.drop_delay);
        thread::sleep(Duration::from_millis(config.drop_delay));
    }

    println!("Shutting off motor for slot {} ({})", slot, slot_config);
    if let Err(err) = run_motor(slot_config, false) {
        eprintln!(
            "Couldn't turn off motor for slot {} ({})! {:?}",
            slot, slot_config, err
        );
        result = Err(err);
    }

    match slot_config {
        OWFS(_) => {
            println!("Drop completed. Allowing another drop time to stop motors again.");
            thread::sleep(Duration::from_millis(config.drop_delay));

            println!("Shutting off motor again to ensure it's safe");
            if let Err(err) = run_motor(slot_config, false) {
                eprintln!(
                    "Couldn't turn off motor [again] for slot {} ({})! {:?}",
                    slot, slot_config, err
                );
                return Err(err);
            }
        }
        GPIO { .. } => {
            println!("Drop completed (GPIO drop, we trust the kernel)");
        }
    };

    println!("Drop transaction finished with {:?}", result);

    result
}
