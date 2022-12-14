mod device;

use clap::{arg, Command};
use std::fs::File;
use std::io::{Read, Write};
use std::num::ParseIntError;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use termios::os::target::B115200;
use termios::{
    cfsetspeed, tcflush, tcsetattr, Termios, BRKINT, CS8, CSIZE, ECHO, ECHONL, ICANON, ICRNL,
    IEXTEN, IGNBRK, IGNCR, INLCR, ISIG, ISTRIP, IXON, OPOST, PARENB, PARMRK, TCIOFLUSH, TCSANOW,
};
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
                )
                .subcommand(
                    Command::new("boot")
                        .about("Boot a raw binary on the device")
                        .arg(arg!(<binary> "The binary file")),
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

            let fd = device.as_raw_fd();
            let mut termios = Termios::from_fd(fd).unwrap();

            cfsetspeed(&mut termios, B115200).unwrap();

            // Set options for "raw" mode (similar to cfmakeraw).
            termios.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
            termios.c_oflag &= !(OPOST);
            termios.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
            termios.c_cflag &= !(CSIZE | PARENB);
            termios.c_cflag |= CS8;

            tcsetattr(fd, TCSANOW, &termios).unwrap();
            tcflush(fd, TCIOFLUSH).unwrap();

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

                    loop {
                        let mut value = [0u8; 1];
                        device.read(&mut value).unwrap();
                        checksum ^= value[0];

                        if remaining > 0 {
                            output.write(&value).unwrap();
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
                Some(("boot", sub_matches)) => {
                    let binary_path = sub_matches.value_of("binary").unwrap();

                    let mut binary = File::options()
                        .read(true)
                        .write(false)
                        .create(false)
                        .truncate(false)
                        .open(binary_path)
                        .unwrap();
                    let mut binary_size = binary.metadata().unwrap().len();

                    device
                        .write(&[b'B', b'O', b'O', b'T', b'F', b'I', b'L', b'E'])
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                    device.write(format!("{:#x}", binary_size).as_bytes()).unwrap();
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

                    loop {
                        let mut value = [0u8; 1];
                        binary.read(&mut value).unwrap();
                        device.write(&value).unwrap();

                        if binary_size % 256 == 0 {
                            // Ensure that the same byte is sent back to confirm that it was received.
                            let mut returned_value = [0u8; 1];
                            device.read(&mut returned_value).unwrap();

                            assert_eq!(value[0], returned_value[0], "Device did not echo back the correct byte");
                        }

                        binary_size -= 1;

                        if binary_size == 0 {
                            break;
                        }
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

                    loop {
                        let mut value = [0u8; 1];
                        device.read(&mut value).unwrap();
                        print!("{}", value[0] as char);
                    }
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
