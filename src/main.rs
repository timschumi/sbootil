use clap::{arg, Command};
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
            let vendor_id =
                match u16::from_str_radix(sub_matches.get_one::<String>("ID").unwrap(), 16) {
                    Ok(vendor_id) => vendor_id,
                    Err(_) => {
                        panic!("Invalid vendor ID")
                    }
                };

            list_devices(vendor_id);
        }
        _ => unreachable!(),
    }
}
