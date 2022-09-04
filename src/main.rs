mod device;

use clap::{arg, Command};
use std::fs::File;
use std::io::{Read, Write};
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
                    arg!(<id> "The vendor ID to filter for")
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
        .subcommand(
            Command::new("bootstub")
                .about("Talking to bootstub")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("dump")
                        .about("Dump memory from the device")
                        .arg(arg!(<start> "The start address"))
                        .arg(arg!(<end> "The end address"))
                        .arg(arg!(<output> "The output file")),
                ),
        )
        .arg(arg!(--device <ID> "The vendor and device ID to communicate with").required(false))
}

fn parse_id(string: &str) -> Result<u16, ParseIntError> {
    u16::from_str_radix(string, 16)
}

fn parse_u64(string: &str) -> Result<u64, ParseIntError> {
    if string.starts_with("0x") || string.starts_with("0X") {
        Ok(u64::from_str_radix(&string[2..], 16))?
    } else {
        Ok(u64::from_str_radix(string, 10))?
    }
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
            let vendor_id = match parse_id(sub_matches.get_one::<String>("id").unwrap()) {
                Ok(vendor_id) => vendor_id,
                Err(_) => {
                    panic!("Invalid vendor ID")
                }
            };

            list_devices(vendor_id);
            return;
        }
        Some(("bootstub", sub_matches)) => {
            let device_path = matches.value_of("device").unwrap();
            let mut device = File::options()
                .read(true)
                .write(true)
                .open(device_path)
                .unwrap();

            // Try the handshake.
            device
                .write(&[b'W', b'H', b'O', b'I', b'S', b'D', b'I', b'S'])
                .unwrap();
            let mut buf = [0u8; 16 * 1024];
            let handshake_end_offset = device.read(&mut buf).unwrap();
            let mut handshake_response = [0u8; 8];
            handshake_response
                .clone_from_slice(&buf[handshake_end_offset - 8..handshake_end_offset]);
            assert_eq!(
                handshake_response,
                [b'B', b'O', b'O', b'T', b'S', b'T', b'U', b'B'],
                "Protocol hello response not as expected: {:?}",
                handshake_response
            );

            match sub_matches.subcommand() {
                Some(("dump", sub_matches)) => {
                    let start_address_str = sub_matches.value_of("start").unwrap();
                    let end_address_str = sub_matches.value_of("end").unwrap();
                    let output_path = sub_matches.value_of("output").unwrap();

                    let start_address = parse_u64(start_address_str).unwrap();
                    let end_address = parse_u64(end_address_str).unwrap();

                    let mut output = File::options()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(output_path)
                        .unwrap();

                    device
                        .write(&[b'U', b'P', b'L', b'D', b'M', b'E', b'M'])
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                    device.write(start_address_str.as_bytes()).unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                    device.write(end_address_str.as_bytes()).unwrap();
                    std::thread::sleep(Duration::from_millis(100));

                    // Ensure that the device accepted the upload.
                    let mut buf = [0u8; 8];
                    device.read(&mut buf).unwrap();
                    assert_eq!(
                        buf[0..8],
                        [b'S', b'T', b'R', b'T', b'U', b'P', b'L', b'D'],
                        "Upload start response not as expected: {:?}",
                        buf
                    );

                    let mut remaining = end_address - start_address;
                    let mut checksum = 0u8;
                    let mut bit_count = 0;
                    let mut sliding_window = 0u16;

                    loop {
                        while bit_count < 8 {
                            let mut value = [0u8; 1];
                            device.read(&mut value).unwrap();

                            sliding_window = (sliding_window << 7) | (value[0] & 0b1111111) as u16;
                            bit_count += 7;
                        }

                        let value = ((sliding_window >> (bit_count - 8)) & 0xff) as u8;
                        bit_count -= 8;

                        checksum ^= value;

                        if remaining > 0 {
                            output.write(&[value]).unwrap();
                        } else {
                            break;
                        }

                        remaining -= 1;
                    }

                    if checksum != 0 {
                        println!("Checksum does not match: {:#02x}", checksum);
                    }

                    // Check end of transfer.
                    let mut buf = [0u8; 7];
                    device.read(&mut buf).unwrap();
                    assert_eq!(
                        buf[0..7],
                        [b'E', b'N', b'D', b'U', b'P', b'L', b'D'],
                        "Upload end response not as expected: {:?}",
                        buf
                    );
                }
                _ => unreachable!(),
            }

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
