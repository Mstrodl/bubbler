use gpio_cdev::{Chip, Line, LineHandle, LineRequestFlags};
use std::env;
use std::fmt::Display;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub enum SlotConfig {
    OWFS(String),
    GPIO {
        vend: LineHandle,
        stocked: LineHandle,
        cam: Option<Line>,
    },
}

impl Display for SlotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OWFS(id) => write!(f, "{}", id),
            Self::GPIO { vend, stocked, cam } => {
                write!(
                    f,
                    "{}.{}{}",
                    vend.line().offset(),
                    stocked.line().offset(),
                    cam.as_ref()
                        .map(|cam| format!(".{}", cam.offset()))
                        .unwrap_or_default()
                )
            }
        }
    }
}

#[allow(dead_code)]
pub struct Latch {
    delete_thread: JoinHandle<()>,
    sender: Sender<Instant>,
}

impl Latch {
    fn new(pin: LineHandle) -> Self {
        let (sender, receiver) = channel::<Instant>();
        let delete_thread = thread::spawn(move || {
            loop {
                let instant = receiver.recv().unwrap();
                let now = Instant::now();
                if now > instant {
                    continue;
                }
                pin.set_value(1).unwrap();
                thread::sleep(instant.duration_since(now));
                while let Ok(instant) = receiver.try_recv() {
                    let now = Instant::now();
                    if now > instant {
                        continue;
                    }
                    // Let this run finish first
                    thread::sleep(instant.duration_since(now));
                }
                pin.set_value(0).unwrap();
            }
        });
        Latch {
            delete_thread,
            sender,
        }
    }
    pub fn open(&self) {
        // No way the motor will spin > 1 minute
        self.sender
            .send(Instant::now() + Duration::from_secs(60))
            .unwrap();
    }
}

pub struct ConfigData {
    pub temperature_id: String,
    pub slots: Vec<SlotConfig>,
    pub latch: Option<Latch>,
    pub drop_delay: u64,
}

fn lookup_pin(spec: &str) -> Result<Line, gpio_cdev::Error> {
    let mut spec = spec.split(':');
    let pin = spec.next().unwrap();
    let chip_id = spec.next().map(|s| s.parse().unwrap()).unwrap_or(0u32);
    let mut chip = Chip::new(format!("/dev/gpiochip{chip_id}"))?;
    chip.get_line(pin.parse().unwrap())
}

impl ConfigData {
    pub fn new() -> ConfigData {
        let mut slots: Vec<SlotConfig> = Vec::new();
        if let Ok(addresses) = env::var("BUB_SLOT_ADDRESSES") {
            let slot_addresses = addresses.split(',');
            for slot in slot_addresses {
                slots.push(SlotConfig::OWFS(slot.to_string()));
            }
        } else {
            let vend = env::var("BUB_VEND_PINS").unwrap();
            let vend = vend.split(',');
            let stocked = env::var("BUB_STOCKED_PINS").unwrap();
            let stocked = stocked.split(',');
            let cam = env::var("BUB_CAM_PINS")
                .ok()
                .into_iter()
                .flat_map(|cam| cam.split(',').map(str::to_string).collect::<Vec<_>>())
                .map(Some);
            let mut input_flags = LineRequestFlags::INPUT;
            if env::var("BUB_ACTIVE_LOW").unwrap_or("0".to_string()) == "1" {
                input_flags |= LineRequestFlags::ACTIVE_LOW
            };
            for ((vend, stocked), cam) in vend.zip(stocked).zip(cam.chain(std::iter::repeat(None)))
            {
                let vend = lookup_pin(vend)
                    .unwrap()
                    .request(LineRequestFlags::OUTPUT, 0, "bubbler-vend")
                    .unwrap();
                let stocked = lookup_pin(stocked)
                    .unwrap()
                    .request(input_flags.clone(), 0, "bubbler-stocked")
                    .unwrap();
                let cam = cam.map(|cam| lookup_pin(&cam).unwrap());
                slots.push(SlotConfig::GPIO { vend, stocked, cam });
            }
        }
        ConfigData {
            temperature_id: env::var("BUB_TEMP_ADDRESS").unwrap(),
            slots,
            latch: env::var("BUB_LATCH_PIN")
                .map(|pin| pin.parse::<u32>().unwrap())
                .map(|pin| {
                    Chip::new("/dev/gpiochip0")
                        .unwrap()
                        .get_line(pin)
                        .unwrap()
                        .request(LineRequestFlags::OUTPUT, 0, "bubbler-latch")
                        .unwrap()
                })
                .map(Latch::new)
                .ok(),
            drop_delay: env::var("BUB_DROP_DELAY").unwrap().parse::<u64>().unwrap(),
        }
    }
}

impl Default for ConfigData {
    fn default() -> ConfigData {
        ConfigData::new()
    }
}

pub struct AppData {
    pub config: Mutex<ConfigData>,
}
