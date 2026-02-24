mod ble;
mod gpio;
mod ibutton;
mod infrared;
mod nfc;
mod rfid;
mod storage;
mod subghz;
mod system;

use super::traits::FlipperModule;

pub fn register_all() -> Vec<Box<dyn FlipperModule>> {
    vec![
        Box::new(system::SystemModule),
        Box::new(subghz::SubGhzModule),
        Box::new(nfc::NfcModule),
        Box::new(rfid::RfidModule),
        Box::new(infrared::InfraredModule),
        Box::new(gpio::GpioModule),
        Box::new(storage::StorageModule),
        Box::new(ibutton::IButtonModule),
        Box::new(ble::BleModule),
    ]
}
