use clap::{arg, Command};
use rusb::{ConfigDescriptor, Direction, Interface};
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
        .subcommand(Command::new("reboot-to-upload").about("Reboot into upload mode"))
        .arg(arg!(--device <ID> "The vendor and device ID to communicate with").required(false))
}

fn parse_id(string: &str) -> Result<u16, ParseIntError> {
    u16::from_str_radix(string, 16)
}

fn find_interface(config_desc: &ConfigDescriptor) -> (Interface, u8, u8, u8) {
    for interface in config_desc.interfaces() {
        for interface_descriptor in interface.descriptors() {
            if interface_descriptor.num_endpoints() == 2
                && interface_descriptor.class_code() == 0x0a
            {
                let mut endpoint_in = None;
                let mut endpoint_out = None;

                for endpoint_descriptor in interface_descriptor.endpoint_descriptors() {
                    if endpoint_descriptor.direction() == Direction::In {
                        endpoint_in = Some(endpoint_descriptor.address());
                    }
                    if endpoint_descriptor.direction() == Direction::Out {
                        endpoint_out = Some(endpoint_descriptor.address());
                    }
                }

                return (
                    interface,
                    interface_descriptor.setting_number(),
                    endpoint_in.unwrap(),
                    endpoint_out.unwrap(),
                );
            }
        }
    }

    unreachable!();
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

    let mut device_handle = rusb::open_device_with_vid_pid(vendor_id, device_id)
        .expect("Device not found or not openable");
    let config_desc = device_handle.device().config_descriptor(0).unwrap();
    let (interface, setting, endpoint_in, endpoint_out) = find_interface(&config_desc);

    device_handle.claim_interface(interface.number()).unwrap();

    device_handle
        .set_alternate_setting(interface.number(), setting)
        .unwrap();

    let mut buf_protocol_hello = [0u8; 4];
    buf_protocol_hello[0] = 0x4f;
    buf_protocol_hello[1] = 0x44;
    buf_protocol_hello[2] = 0x49;
    buf_protocol_hello[3] = 0x4e;

    device_handle
        .write_bulk(endpoint_out, &buf_protocol_hello, Duration::from_secs(1))
        .unwrap();

    let mut buf_protocol_hello_response = [0u8; 7];

    device_handle
        .read_bulk(
            endpoint_in,
            &mut buf_protocol_hello_response,
            Duration::from_secs(1),
        )
        .unwrap();

    let mut buf_start_session = [0u8; 1024];
    buf_start_session[0] = 0x64;
    buf_start_session[1] = 0x00;

    device_handle
        .write_bulk(endpoint_out, &buf_start_session, Duration::from_secs(1))
        .unwrap();

    let mut response = [0u8; 1024];

    device_handle
        .read_bulk(endpoint_in, &mut response, Duration::from_secs(1))
        .unwrap();

    let mut buf_end_session = [0u8; 1024];
    buf_end_session[0] = 0x67;
    buf_end_session[4] = 0x01;

    device_handle
        .write_bulk(endpoint_out, &buf_end_session, Duration::from_secs(1))
        .unwrap();

    response = [0u8; 1024];

    device_handle
        .read_bulk(endpoint_in, &mut response, Duration::from_secs(1))
        .unwrap();

    device_handle.release_interface(interface.number()).unwrap();
}
