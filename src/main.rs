mod bus;
mod cartridge;
mod cpu;
mod interrupt;
mod joypad;
mod ppu;
mod timer;

use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;
use std::env;
use std::fs;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;
use std::time::{Duration, Instant};

fn main() {
    // Read command-line arguments
    let args: Vec<String> = env::args().collect();

    // Verify that a ROM file path was provided
    if args.len() < 2 {
        eprintln!("Usage: {} <rom_path.gb>", args.get(0).map(|s| s.as_str()).unwrap_or("GB_Emulator"));
        eprintln!("Example: cargo run -- roms/dmg-acid2.gb");
        return;
    }

    let rom_path = &args[1];

    // Read all bytes from the ROM file into memory
    let rom_bytes = match fs::read(rom_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("Error reading file {}: {}", rom_path, err);
            return;
        }
    };

    // Create the cartridge by parsing its header
    let cartridge = match Cartridge::new(rom_bytes) {
        Ok(cart) => cart,
        Err(err) => {
            eprintln!("Error parsing cartridge: {}", err);
            return;
        }
    };

    println!("=== Game Boy Cartridge Info ===");
    println!("File:             {}", rom_path);
    println!("Title:            {}", cartridge.header.title);
    println!("Cartridge Type:   {:?}", cartridge.header.cartridge_type);
    println!("ROM Size:         {} KiB", cartridge.header.rom_size / 1024);
    println!("RAM Size:         {} KiB", cartridge.header.ram_size / 1024);
    println!(
        "Header Checksum:  {:#04X} ({})",
        cartridge.header.header_checksum,
        if cartridge.header.is_checksum_valid { "VALID" } else { "INVALID!" }
    );

    println!("\n=== Starting Emulator Execution ===\n");

    const GB_WIDTH: u32 = 160;
    const GB_HEIGHT: u32 = 144;
    const SCALE: f64 = 3.0;

    const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title(format!("Game Boy - {}", cartridge.header.title))
        .with_inner_size(LogicalSize::new(GB_WIDTH as f64 * SCALE, GB_HEIGHT as f64 * SCALE))
        .with_min_inner_size(LogicalSize::new(GB_WIDTH as f64, GB_HEIGHT as f64))
        .build(&event_loop)
        .unwrap();

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels = Pixels::new(GB_WIDTH, GB_HEIGHT, surface_texture).unwrap();

    let mut bus = Bus::new(cartridge);
    let mut cpu = Cpu::new();
    let mut last_frame_time = Instant::now();

    event_loop.run(move |event, elwt| {
        match event {
            // Window close button clicked
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                elwt.exit();
            }
            // Window resized by user or OS
            Event::WindowEvent { event: WindowEvent::Resized(new_size), .. } => {
                pixels.resize_surface(new_size.width, new_size.height).unwrap();
            }
            // Window requests next frame redraw
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // Run CPU, Timer, and PPU cycles until an entire frame (144 scanlines) is complete
                while !bus.ppu.frame_ready {
                    let cycles = cpu.step(&mut bus);
                    bus.timer.step(cycles, &mut bus.if_flag);
                    bus.ppu.step(cycles, &mut bus.if_flag);
                }
                bus.ppu.frame_ready = false;

                // Copy framebuffer to pixels buffer
                pixels.frame_mut().copy_from_slice(&bus.ppu.framebuffer);

                // Render frame onto the GPU
                if let Err(err) = pixels.render() {
                    eprintln!("Rendering error: {}", err);
                    elwt.exit();
                }

                // Sleep to maintain 60 FPS
                let elapsed = last_frame_time.elapsed();
                if elapsed < FRAME_DURATION {
                    std::thread::sleep(FRAME_DURATION - elapsed);
                }
                last_frame_time = Instant::now();
            }
            
            // Request redraw on idle to keep the emulator running at display refresh rate
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).unwrap();
}
