// Copyright (C) 2026 Daniel Kurdoghlian
// Pushing Pixels, Vienna <https://pushingpixels.at>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
// for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, see <https://www.gnu.org/licenses>.
//
// Additional permission under GNU GPL version 3 section 7
//
// If you modify this Program, or any covered work, by linking or combining
// it with NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as
// libcudart, libcublas and libcublasLt, and the CUDA driver library libcuda
// of the NVIDIA driver) (or a modified version of that library), containing
// parts covered by the terms of the NVIDIA CUDA Toolkit End User License
// Agreement or the NVIDIA Driver License Agreement, the licensors of this
// Program grant you additional permission to convey the resulting work.

use anyhow::Result;
use clap::{Parser, Subcommand};
use dictate::config::Config;
use dictate::engine::{DecodeOpts, Engine, Transcriber};
use dictate::{daemon, simulate, wav};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

// Nur noch der Dienst und der Offline-Test: Oberfläche, Kürzel, Textausgabe und Textaktionen sind `dictate-ui`
// (spricht den Socket direkt), Einstellungen `dictate-settings`. Die Clients für AwesomeWM (`start`, `listen`, `act`,
// `palette`, `diag`, `paste` …) entfielen mit W7 (PLAN-wayland.md 6f). Kein Doc-Kommentar: clap zeigte ihn in --help.
#[derive(Parser)]
#[command(name = "dictate", version, about = "Local live dictation (whisper.cpp, CUDA)")]
struct Cli {
    /// Config file (default: ~/.config/dictate/config.toml)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Sub,
}

#[derive(Subcommand)]
enum Sub {
    /// Run the daemon in the foreground (for systemd)
    Daemon,
    /// Transcribe a 16 kHz mono WAV file (offline test)
    Transcribe {
        file: PathBuf,
        /// Feed the file through the streamer in 20 ms blocks and print events
        #[arg(long)]
        stream: bool,
        /// With --stream: pace the input in real time instead of a virtual clock
        #[arg(long, requires = "stream")]
        realtime: bool,
        /// Batch mode only: override the encoder context in frames (1500 = full 30 s)
        #[arg(long, value_name = "N", conflicts_with = "stream")]
        audio_ctx: Option<i32>,
    },
}

fn init_tracing() {
    let default = if std::env::var_os("DICTATE_DEBUG").is_some() { "debug" } else { "warn" };
    let filter = tracing_subscriber::EnvFilter::try_from_env("DICTATE_LOG").unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
}

fn transcribe_batch(cfg: &Config, file: &std::path::Path, audio_ctx: Option<i32>) -> Result<()> {
    let pcm = wav::read_16k_mono(file)?;
    let t_load = Instant::now();
    let mut engine = Engine::load(cfg)?;
    let load = t_load.elapsed();
    let prompt = dictate::pipeline::vocabulary::prompt_base(cfg);
    let opts = DecodeOpts { audio_ctx, beam_size: cfg.final_beam_size, prompt: &prompt, single_segment: false };
    let t_run = Instant::now();
    let segs = engine.transcribe(&pcm, &opts)?;
    let run = t_run.elapsed();
    let text = segs.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    println!("{text}");
    eprintln!("audio {:.1} s, load {} ms, transcribe {} ms", pcm.len() as f64 / 16000.0, load.as_millis(), run.as_millis());
    Ok(())
}

fn run(cli: Cli) -> Result<i32> {
    let cfg = Config::load(cli.config.as_deref())?;
    Ok(match cli.command {
        Sub::Daemon => {
            daemon::run(cfg, Some(cli.config.unwrap_or_else(Config::default_path)))?;
            0
        }
        Sub::Transcribe { file, stream, realtime, audio_ctx } => {
            if stream {
                simulate::run(&cfg, &file, realtime)?;
            } else {
                transcribe_batch(&cfg, &file, audio_ctx)?;
            }
            0
        }
    })
}

fn main() -> ExitCode {
    init_tracing();
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("dictate: {e:#}");
            ExitCode::from(1)
        }
    }
}
