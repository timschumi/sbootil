use rusb::{DeviceHandle, Direction, GlobalContext};
use std::error::Error;
use std::time::Duration;

pub(crate) struct UsbCdcDevice {
    handle: DeviceHandle<GlobalContext>,
    interface: u8,
    setting: u8,
    endpoint_in: u8,
    endpoint_out: u8,
}

impl UsbCdcDevice {
    pub(crate) fn from_handle(handle: DeviceHandle<GlobalContext>) -> Result<Self, Box<dyn Error>> {
        let config_descriptor = handle.device().config_descriptor(0)?;

        for interface in config_descriptor.interfaces() {
            for interface_descriptor in interface.descriptors() {
                if interface_descriptor.num_endpoints() != 2 {
                    continue;
                }

                if interface_descriptor.class_code() != 0x0a {
                    continue;
                }

                let endpoint_in = interface_descriptor
                    .endpoint_descriptors()
                    .filter(|ed| ed.direction() == Direction::In)
                    .next()
                    .unwrap()
                    .address();

                let endpoint_out = interface_descriptor
                    .endpoint_descriptors()
                    .filter(|ed| ed.direction() == Direction::Out)
                    .next()
                    .unwrap()
                    .address();

                return Ok(Self {
                    handle,
                    interface: interface.number(),
                    setting: interface_descriptor.setting_number(),
                    endpoint_in,
                    endpoint_out,
                });
            }
        }

        Err("No matching interface found")?
    }

    pub(crate) fn setup_interface(&mut self) -> Result<(), Box<dyn Error>> {
        self.handle.claim_interface(self.interface)?;

        self.handle
            .set_alternate_setting(self.interface, self.setting)?;

        Ok(())
    }

    pub(crate) fn teardown_interface(&mut self) -> Result<(), Box<dyn Error>> {
        self.handle.release_interface(self.interface)?;

        Ok(())
    }

    pub(crate) fn write(&self, buf: &[u8], timeout: Duration) -> Result<usize, Box<dyn Error>> {
        let transferred = self.handle.write_bulk(self.endpoint_out, buf, timeout)?;

        Ok(transferred)
    }

    pub(crate) fn write_packet(
        &self,
        buf: &[u8],
        size: usize,
        timeout: Duration,
    ) -> Result<usize, Box<dyn Error>> {
        let mut packet = vec![0u8; size];
        packet[0..buf.len()].clone_from_slice(buf);

        self.write(&packet, timeout)
    }

    pub(crate) fn read(&self, buf: &mut [u8], timeout: Duration) -> Result<usize, Box<dyn Error>> {
        let transferred = self.handle.read_bulk(self.endpoint_in, buf, timeout)?;

        Ok(transferred)
    }
}
