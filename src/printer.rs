//! High-level flows, ported from `SP3.getPrinterInformation` / `SP3.printPhoto`.
//! The printer wants several short connections, not one long one, so each phase
//! opens and closes its own socket with a one-second pause in between.

use crate::client::Client;
use crate::image_enc::{Model, SEGMENT_LEN};
use crate::packet::{Command, RTN_OK};
use anyhow::{bail, Result};
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug)]
pub struct Info {
    pub model: String,
    pub firmware: String,
    pub hardware: String,
    pub battery: u8,
    pub prints_left: u8,
    pub total_prints: u32,
    pub max_width: u16,
    pub max_height: u16,
}

fn with_connection<T>(c: &mut Client, f: impl FnOnce(&mut Client) -> Result<T>) -> Result<T> {
    c.connect()?;
    let r = f(c);
    c.close();
    r
}

/// Read-only: queries version, model, specifications and print count.
pub fn get_info(c: &mut Client) -> Result<Info> {
    with_connection(c, |c| {
        let ver = c.send(&Command::Version)?;
        let model = c.send(&Command::ModelName)?;
        let specs = c.send(&Command::Specifications)?;
        let count = c.send(&Command::PrintCount)?;
        let v = ver.version();
        let s = specs.specifications();
        Ok(Info {
            model: model.model_name(),
            firmware: v.firmware,
            hardware: v.hardware,
            battery: ver.battery,
            prints_left: ver.prints_left,
            total_prints: count.print_count(),
            max_width: s.max_width,
            max_height: s.max_height,
        })
    })
}

/// Sends the image and starts printing. Uses one sheet of film.
/// Returns `Ok(false)` if the printer never reported completion within 30 s.
pub fn print(
    c: &mut Client,
    model: Model,
    data: &[u8],
    mut progress: impl FnMut(u8, &str),
) -> Result<bool> {
    if data.len() != model.image_len() {
        bail!("image data is {} bytes, {} needs {}", data.len(), model.name(), model.image_len());
    }
    let pause = || sleep(Duration::from_secs(1));

    progress(10, "Sending pre-print commands");
    with_connection(c, |c| {
        for n in 1..=8u16 {
            c.send(&Command::PrePrint(n))?;
        }
        Ok(())
    })?;

    pause();
    progress(20, "Locking printer");
    with_connection(c, |c| c.send(&Command::Lock(1)).map(|_| ()))?;

    pause();
    progress(30, "Resetting printer");
    with_connection(c, |c| c.send(&Command::Reset).map(|_| ()))?;

    pause();
    progress(40, "About to send image");
    let segments = data.len() / SEGMENT_LEN;
    with_connection(c, |c| {
        c.send(&Command::PrepImage { format: 16, options: 0, img_len: data.len() as u32 })?;
        for (seq, chunk) in data.chunks(SEGMENT_LEN).enumerate() {
            c.send(&Command::SendImage { seq: seq as u32, data: chunk })?;
            let pct = 40 + (seq + 1) * 30 / segments;
            progress(pct as u8, &format!("Sent image segment {}/{}", seq + 1, segments));
        }
        c.send(&Command::Type83)?;
        Ok(())
    })?;

    pause();
    progress(90, "Waiting for print to finish");
    let done = with_connection(c, |c| {
        c.send(&Command::LockState)?;
        c.send(&Command::Version)?;
        c.send(&Command::ModelName)?;
        for _ in 0..30 {
            if c.send(&Command::Type195)?.return_code == RTN_OK {
                return Ok(true);
            }
            sleep(Duration::from_secs(1));
        }
        Ok(false)
    })?;
    progress(100, if done { "Print is complete" } else { "Timed out waiting for print" });
    Ok(done)
}
