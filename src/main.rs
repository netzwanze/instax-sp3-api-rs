mod client;
mod image_enc;
mod packet;
mod printer;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use client::Client;
use image_enc::Model;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Print to a Fujifilm Instax SP-2 / SP-3 over its Wi-Fi network")]
struct Cli {
    /// Printer address (the printer is the gateway of its own Wi-Fi network).
    #[arg(long, global = true, default_value = "192.168.0.251")]
    host: String,
    #[arg(long, global = true, default_value_t = 8080)]
    port: u16,
    /// Printer PIN.
    #[arg(short = 'i', long, global = true, default_value_t = 1111)]
    pin: u16,
    /// Printer model.
    #[arg(short = 'm', long, global = true, value_enum, default_value = "3")]
    model: ModelArg,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModelArg {
    #[value(name = "2")]
    Sp2,
    #[value(name = "3")]
    Sp3,
}

impl From<ModelArg> for Model {
    fn from(m: ModelArg) -> Self {
        match m {
            ModelArg::Sp2 => Model::Sp2,
            ModelArg::Sp3 => Model::Sp3,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Query the printer (read-only, prints nothing).
    Info,
    /// Convert an image and save the result as PNG, without touching the printer.
    Preview { image: PathBuf, out: PathBuf },
    /// Print an image. USES ONE SHEET OF FILM.
    Print {
        image: PathBuf,
        /// Convert and encode only; do not connect to the printer.
        #[arg(long)]
        dry_run: bool,
        /// Print even if the printer reports a different model than --model.
        #[arg(long)]
        force: bool,
    },
}

fn progress_bar(pct: u8, status: &str) {
    let filled = pct as usize * 60 / 100;
    eprint!("\r[{}{}] {:>3}% {:<44}", "=".repeat(filled), "-".repeat(60 - filled), pct, status);
    if pct == 100 {
        eprintln!();
    }
}

fn show_info(i: &printer::Info) {
    println!("Model:            {}", i.model);
    println!("Firmware:         {}", i.firmware);
    println!("Hardware:         {}", i.hardware);
    println!("Battery (0-7):    {}", i.battery);
    println!("Prints remaining: {}", i.prints_left);
    println!("Lifetime prints:  {}", i.total_prints);
    println!("Max image size:   {}x{}", i.max_width, i.max_height);
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let model: Model = cli.model.into();
    let mut client = Client::new(&cli.host, cli.port, cli.pin);

    match cli.cmd {
        Cmd::Info => show_info(&printer::get_info(&mut client)?),
        Cmd::Preview { image, out } => {
            let img = image_enc::load_and_convert(&image, model)?;
            img.save(&out)?;
            println!("Saved {}x{} preview to {}", img.width(), img.height(), out.display());
        }
        Cmd::Print { image, dry_run, force } => {
            // Fail on a bad image before touching the printer.
            let fitted = image_enc::load_and_convert(&image, model)?;
            let data = image_enc::encode(&fitted, model)?;
            if dry_run {
                println!(
                    "Dry run: {} image encoded to {} bytes in {} segments of {}; nothing sent.",
                    model.name(),
                    data.len(),
                    model.segments(),
                    image_enc::SEGMENT_LEN
                );
                return Ok(());
            }
            let info = printer::get_info(&mut client)?;
            show_info(&info);
            if !info.model.to_uppercase().contains(model.name()) && !force {
                bail!(
                    "printer reports model {:?} but --model selects {}; use --force to print anyway",
                    info.model,
                    model.name()
                );
            }
            if info.prints_left == 0 {
                bail!("printer reports 0 prints remaining (is film loaded?)");
            }
            eprintln!("Printing {} (one sheet of film)...", image.display());
            if !printer::print(&mut client, model, &data, progress_bar)? {
                bail!("the printer did not report completion in time; check it before retrying");
            }
        }
    }
    Ok(())
}
