use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
struct HidpCaps {
    usage: u16,
    usage_page: u16,
    input_report_byte_length: u16,
    output_report_byte_length: u16,
    feature_report_byte_length: u16,
    reserved: [u16; 17],
    number_link_collection_nodes: u16,
    number_input_button_caps: u16,
    number_input_value_caps: u16,
    number_input_data_indices: u16,
    number_output_button_caps: u16,
    number_output_value_caps: u16,
    number_output_data_indices: u16,
    number_feature_button_caps: u16,
    number_feature_value_caps: u16,
    number_feature_data_indices: u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut c_void,
    ) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn CreateEventW(
        security_attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn ResetEvent(event: *mut c_void) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn ReadFile(
        file: *mut c_void,
        buffer: *mut u8,
        number_of_bytes_to_read: u32,
        number_of_bytes_read: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn WriteFile(
        file: *mut c_void,
        buffer: *const u8,
        number_of_bytes_to_write: u32,
        number_of_bytes_written: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(
        file: *mut c_void,
        overlapped: *mut Overlapped,
        number_of_bytes_transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: *mut c_void, overlapped: *mut Overlapped) -> i32;
    fn GetLastError() -> u32;
}

#[link(name = "hid")]
unsafe extern "system" {
    fn HidD_GetPreparsedData(
        hid_device_object: *mut c_void,
        preparsed_data: *mut *mut c_void,
    ) -> i32;
    fn HidD_FreePreparsedData(preparsed_data: *mut c_void) -> i32;
    fn HidP_GetCaps(preparsed_data: *mut c_void, capabilities: *mut HidpCaps) -> i32;
    fn HidD_GetFeature(
        hid_device_object: *mut c_void,
        report_buffer: *mut u8,
        report_buffer_length: u32,
    ) -> i32;
    fn HidD_SetFeature(
        hid_device_object: *mut c_void,
        report_buffer: *const u8,
        report_buffer_length: u32,
    ) -> i32;
}

const ERROR_IO_PENDING: u32 = 997;
const WAIT_OBJECT_0: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Wired,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryReading {
    pub percentage: Option<u8>,
    pub charging: bool,
    pub mode: ConnectionKind,
}

pub struct KeyboardReader {
    kind: ConnectionKind,
    handle: *mut c_void,
    event: *mut c_void,
}

unsafe impl Send for KeyboardReader {}

fn open_hid_handle(path_str: &str, overlapped: bool) -> Option<*mut c_void> {
    let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
    let flags = if overlapped { 0x40000000 } else { 0 };
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0x80000000 | 0x40000000,
            1 | 2,
            null_mut(),
            3,
            flags,
            null_mut(),
        )
    };
    if handle as isize == -1 || handle.is_null() {
        None
    } else {
        Some(handle)
    }
}

fn feature_report_len(handle: *mut c_void) -> u16 {
    unsafe {
        let mut pp: *mut c_void = null_mut();
        if HidD_GetPreparsedData(handle, &mut pp) == 0 || pp.is_null() {
            return 0;
        }
        let mut caps: HidpCaps = zeroed();
        let ok = HidP_GetCaps(pp, &mut caps);
        HidD_FreePreparsedData(pp);
        if ok >= 0 {
            caps.feature_report_byte_length
        } else {
            0
        }
    }
}

impl KeyboardReader {
    pub fn kind(&self) -> ConnectionKind {
        self.kind
    }

    pub fn is_wired_present() -> bool {
        let Ok(api) = hidapi::HidApi::new() else {
            return false;
        };
        api.device_list().any(|d| {
            d.vendor_id() == 0x258A
                && d.product_id() == 0x010C
                && d.usage_page() == 0xFF00
                && d.usage() == 0x0001
        })
    }

    pub fn open() -> Option<Self> {
        let api = hidapi::HidApi::new().ok()?;

        // 1. Prefer wired USB-C (VID_258A & PID_010C, 520-byte Feature Report 0x06).
        // When cable is plugged in, the 2.4 GHz dongle stops responding due to firmware mutex.
        for dev in api.device_list() {
            if dev.vendor_id() == 0x258A
                && dev.product_id() == 0x010C
                && dev.usage_page() == 0xFF00
                && dev.usage() == 0x0001
            {
                if let Ok(path_str) = dev.path().to_str() {
                    if let Some(handle) = open_hid_handle(path_str, false) {
                        if feature_report_len(handle) == 520 {
                            let reader = Self {
                                kind: ConnectionKind::Wired,
                                handle,
                                event: null_mut(),
                            };
                            // Verify this collection actually responds to GET_INFO (0x82)
                            if reader.wired_read(0x82, 0x01, 6).is_some() {
                                return Some(reader);
                            }
                        } else {
                            unsafe { CloseHandle(handle); }
                        }
                    }
                }
            }
        }

        // 2. Fallback to 2.4 GHz wireless dongle (VID_3554 & PID_FA09, 20-byte report 0x13)
        let target = api.device_list().find(|d| {
            d.vendor_id() == 0x3554
                && d.product_id() == 0xFA09
                && d.usage_page() == 0xFF02
                && d.usage() == 0x0002
        })?;

        let path_str = target.path().to_str().ok()?;
        let handle = open_hid_handle(path_str, true)?;
        let event = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
        if event.is_null() {
            unsafe { CloseHandle(handle); }
            return None;
        }

        Some(Self {
            kind: ConnectionKind::Wireless,
            handle,
            event,
        })
    }

    fn wired_read(&self, code: u8, param: u8, want: u16) -> Option<Vec<u8>> {
        let mut req = [0u8; 520];
        req[0] = 0x06;
        req[1] = code;
        req[2] = param;
        req[4] = 1; // total packets
        req[5] = 0; // packet index
        req[6] = (want & 0xFF) as u8;
        req[7] = (want >> 8) as u8;

        sleep(Duration::from_millis(15));
        if unsafe { HidD_SetFeature(self.handle, req.as_ptr(), 520) } == 0 {
            return None;
        }
        sleep(Duration::from_millis(30));

        let mut resp = [0u8; 520];
        resp[0] = 0x06;
        if unsafe { HidD_GetFeature(self.handle, resp.as_mut_ptr(), 520) } == 0 {
            return None;
        }
        if resp[1] != code {
            return None;
        }
        let len = (want as usize).min(512);
        Some(resp[8..8 + len].to_vec())
    }

    fn write(&self, buffer: &[u8], timeout_ms: u32) -> bool {
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: self.event,
        };
        unsafe { ResetEvent(self.event); }
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                self.handle,
                buffer.as_ptr(),
                buffer.len() as u32,
                &mut written,
                &mut ov,
            )
        };
        if ok != 0 {
            return written > 0;
        }
        let err = unsafe { GetLastError() };
        if err != ERROR_IO_PENDING {
            return false;
        }

        let wait = unsafe { WaitForSingleObject(self.event, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            unsafe { CancelIoEx(self.handle, &mut ov); }
            return false;
        }

        let res = unsafe { GetOverlappedResult(self.handle, &mut ov, &mut written, 0) };
        res != 0 && written > 0
    }

    fn read(&self, length: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; length];
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: self.event,
        };
        unsafe { ResetEvent(self.event); }
        let mut bytes_read = 0u32;
        let ok = unsafe {
            ReadFile(
                self.handle,
                buffer.as_mut_ptr(),
                length as u32,
                &mut bytes_read,
                &mut ov,
            )
        };
        if ok != 0 {
            buffer.truncate(bytes_read as usize);
            return Some(buffer);
        }
        let err = unsafe { GetLastError() };
        if err != ERROR_IO_PENDING {
            return None;
        }

        let wait = unsafe { WaitForSingleObject(self.event, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            unsafe { CancelIoEx(self.handle, &mut ov); }
            return None;
        }

        let res = unsafe { GetOverlappedResult(self.handle, &mut ov, &mut bytes_read, 0) };
        if res != 0 && bytes_read > 0 {
            buffer.truncate(bytes_read as usize);
            Some(buffer)
        } else {
            None
        }
    }

    pub fn query_battery(&self) -> Option<BatteryReading> {
        match self.kind {
            ConnectionKind::Wired => {
                let d = self.wired_read(0x87, 0x02, 2)?;
                let pct = d[0];
                let flags = d[1];
                let charging = (flags & 0x10) != 0;
                // On Hi75C Pro wired USB, firmware returns 0 for pct while charging.
                let percentage = if (1..=100).contains(&pct) {
                    Some(pct)
                } else {
                    None
                };
                Some(BatteryReading {
                    percentage,
                    charging,
                    mode: ConnectionKind::Wired,
                })
            }
            ConnectionKind::Wireless => {
                let mut req = [0u8; 20];
                req[0] = 0x13;
                req[1] = 0xCA;
                req[19] = calculate_checksum(&req);

                for _ in 0..4 {
                    if self.write(&req, 200) {
                        sleep(Duration::from_millis(25));
                        if let Some(resp) = self.read(20, 250) {
                            if resp.len() >= 20
                                && resp[0] == 0x13
                                && (resp[1] & 0x7F) == 0x4A
                                && resp[19] == calculate_checksum(&resp)
                            {
                                let pct = resp[5];
                                let charging = (resp[6] & 0x10) != 0;
                                let percentage = if (1..=100).contains(&pct) {
                                    Some(pct)
                                } else {
                                    None
                                };
                                return Some(BatteryReading {
                                    percentage,
                                    charging,
                                    mode: ConnectionKind::Wireless,
                                });
                            }
                        }
                    }
                    sleep(Duration::from_millis(40));
                }
                None
            }
        }
    }
}

impl Drop for KeyboardReader {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() && self.handle as isize != -1 {
                CloseHandle(self.handle);
            }
            if !self.event.is_null() {
                CloseHandle(self.event);
            }
        }
    }
}

fn calculate_checksum(buf: &[u8]) -> u8 {
    let mut sum: u32 = 0;
    for &b in &buf[..19] {
        sum = sum.wrapping_add(b as u32);
    }
    (sum & 0xFF) as u8
}
