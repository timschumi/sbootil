mod device;

use clap::{arg, Command};
use std::num::ParseIntError;
use std::time::Duration;
use usb_ids::FromId;

fn cli() -> Command<'static> {
    Command::new("sbootil")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("list-devices")
                .about("Lists all connected USB devices")
                .arg(
                    arg!(<ID> "The vendor ID to filter for")
                        .required(false)
                        .default_value(&"04e8"),
                ),
        )
        .subcommand(
            Command::new("download")
                .about("Talking to Download Mode")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(Command::new("reboot").about("Reboot the device")),
        )
        .arg(arg!(--device <ID> "The vendor and device ID to communicate with").required(false))
}

fn parse_id(string: &str) -> Result<u16, ParseIntError> {
    u16::from_str_radix(string, 16)
}

fn list_devices(vendor_id: u16) {
    for device in rusb::devices().unwrap().iter() {
        let device_desc = device.device_descriptor().unwrap();

        if device_desc.vendor_id() != vendor_id {
            continue;
        }

        let vendor_name = match usb_ids::Vendor::from_id(device_desc.vendor_id()) {
            Some(vendor) => vendor.name(),
            None => "Unknown vendor",
        };

        let product_name = match usb_ids::Device::from_vid_pid(
            device_desc.vendor_id(),
            device_desc.product_id(),
        ) {
            Some(product) => product.name(),
            None => "Unknown product",
        };

        println!(
            "[{:04x}:{:04x}] {}, {}",
            device_desc.vendor_id(),
            device_desc.product_id(),
            vendor_name,
            product_name,
        );
    }
}

fn main() {
    let matches = cli().get_matches();

    match matches.subcommand() {
        Some(("list-devices", sub_matches)) => {
            let vendor_id = match parse_id(sub_matches.get_one::<String>("ID").unwrap()) {
                Ok(vendor_id) => vendor_id,
                Err(_) => {
                    panic!("Invalid vendor ID")
                }
            };

            list_devices(vendor_id);
            return;
        }
        _ => {}
    }

    let mut id_split = matches.value_of("device").unwrap().split(':');

    let vendor_id = match parse_id(id_split.next().unwrap()) {
        Ok(id) => id,
        Err(_) => {
            panic!("Invalid vendor ID")
        }
    };

    let device_id = match u16::from_str_radix(id_split.next().unwrap(), 16) {
        Ok(id) => id,
        Err(_) => {
            panic!("Invalid device ID")
        }
    };

    let device_handle = rusb::open_device_with_vid_pid(vendor_id, device_id)
        .expect("Device not found or not openable");

    let mut device = device::UsbCdcDevice::from_handle(device_handle).unwrap();

    device.setup_interface().unwrap();

    match matches.subcommand() {
        Some(("download", sub_matches)) => {
            device
                .write(&[0x4f, 0x44, 0x49, 0x4e], Duration::from_secs(1))
                .unwrap();

            let mut hello_response = [0u8; 4];

            device
                .read(&mut hello_response, Duration::from_secs(1))
                .unwrap();

            assert_eq!(
                hello_response[0..4],
                [0x4C, 0x4F, 0x4B, 0x45],
                "Protocol hello response not as expected: {:?}",
                hello_response
            );

            device
                .write_packet(
                    &[0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
                    1024,
                    Duration::from_secs(1),
                )
                .unwrap();

            device
                .read(&mut [0u8; 1024], Duration::from_secs(1))
                .unwrap();

            match sub_matches.subcommand() {
                Some(("reboot", _)) => {
                    // Does nothing, we will reboot at the end of the session anyways.
                }
                _ => unreachable!(),
            }

            device
                .write_packet(
                    &[0x67, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00],
                    1024,
                    Duration::from_secs(1),
                )
                .unwrap();

            device
                .read(&mut [0u8; 1024], Duration::from_secs(1))
                .unwrap();
        }
        _ => unreachable!(),
    }

    device.teardown_interface().unwrap();
}
