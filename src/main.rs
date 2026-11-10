mod bus;
mod cartridge;
mod cpu;
mod joypad;
mod ppu;
mod timer;

use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;
use gilrs::{Axis, Button, EventType, Gilrs};
use joypad::JoypadKey;
use pixels::{PixelsBuilder, SurfaceTexture, ScalingMode};
use pixels::wgpu::BlendState;
use std::env;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

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
    let mut cartridge = match Cartridge::new(rom_bytes) {
        Ok(cart) => cart,
        Err(err) => {
            eprintln!("Error parsing cartridge: {}", err);
            return;
        }
    };

    let sav_path = std::path::Path::new(rom_path).with_extension("sav");

    // Load existing .sav save file if present
    if cartridge.has_battery() {
        match cartridge.battery_load_save(&sav_path) {
            Ok(true) => println!("Loaded save file: {}", sav_path.display()),
            Ok(false) => println!("No existing save file found (a new one will be created when saving)."),
            Err(err) => eprintln!("Warning: failed to load save file {}: {}", sav_path.display(), err),
        }
    }

    println!("=== Game Boy Cartridge Info ===");
    println!("File:             {}", rom_path);
    println!("Title:            {}", cartridge.header.title);
    println!("Cartridge Type:   {:?}", cartridge.header.cartridge_type);
    println!("ROM Size:         {} KiB", cartridge.header.rom_size / 1024);
    println!("RAM Size:         {} KiB", cartridge.header.ram_size / 1024);
    println!("Battery:          {}", if cartridge.has_battery() { "YES" } else { "NO" });
    if cartridge.has_battery() {
        println!("Save File:        {}", sav_path.display());
    }
    println!(
        "Header Checksum:  {:#04X} ({})",
        cartridge.header.header_checksum,
        if cartridge.header.is_checksum_valid { "VALID" } else { "INVALID!" }
    );

    println!("\n=== Keyboard Controls ===");
    println!("D-Pad:  Arrow Keys or W / A / S / D");
    println!("A / B:  Z / X or J / K");
    println!("Select: Shift or Backspace");
    println!("Start:  Enter or Space");

    println!("\n=== Gamepad Controls ===");
    println!("Movement: D-Pad or Left Analog Stick");
    println!("A:        South (Xbox A / PS Cross) or North (Y / Triangle)");
    println!("B:        East (Xbox B / PS Circle) or West (X / Square)");
    println!("Start:    Start button");
    println!("Select:   Select / Back / Share button");

    println!("\n=== Starting Emulator Execution ===\n");

    const GB_WIDTH: u32 = 160;
    const GB_HEIGHT: u32 = 144;
    const SCALE: f64 = 3.0;

    const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(format!("Game Boy - {}", cartridge.header.title))
            .with_inner_size(LogicalSize::new(GB_WIDTH as f64 * SCALE, GB_HEIGHT as f64 * SCALE))
            .with_min_inner_size(LogicalSize::new(GB_WIDTH as f64, GB_HEIGHT as f64))
            .build(&event_loop)
            .unwrap(),
    );

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, Arc::clone(&window));

    let mut pixels = PixelsBuilder::new(GB_WIDTH, GB_HEIGHT, surface_texture)
        .blend_state(BlendState::REPLACE)
        .build()
        .unwrap();

    pixels.resize_buffer(GB_WIDTH, GB_HEIGHT).unwrap();
    pixels.resize_surface(window_size.width, window_size.height).unwrap();

    pixels.set_scaling_mode(ScalingMode::Fill);

    let mut bus = Bus::new(cartridge);
    let mut cpu = Cpu::new();
    let mut last_frame_time = Instant::now();

    // Initialize Gamepad subsystem (gilrs)
    let mut gilrs = match Gilrs::new() {
        Ok(g) => Some(g),
        Err(err) => {
            eprintln!("Warning: could not initialize gamepad subsystem: {}", err);
            None
        }
    };

    let mut stick_left = false;
    let mut stick_right = false;
    let mut stick_up = false;
    let mut stick_down = false;
    const STICK_DEADZONE: f32 = 0.5;

    event_loop.run(move |event, elwt| {
        match event {
            // Window close button clicked
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                match bus.cartridge.battery_save(&sav_path) {
                    Ok(true) => println!("Saved game to {}", sav_path.display()),
                    Ok(false) => {},
                    Err(err) => eprintln!("Error saving game to {}: {}", sav_path.display(), err),
                }
                elwt.exit();
            }
            // Keyboard input events
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput {
                    event: KeyEvent {
                        physical_key: PhysicalKey::Code(key_code),
                        state,
                        repeat,
                        ..
                    },
                    ..
                },
                ..
            } => {
                if !repeat {
                    if let Some(key) = map_key_code(key_code) {
                        match state {
                            ElementState::Pressed => bus.joypad.key_down(key, &mut bus.if_flag),
                            ElementState::Released => bus.joypad.key_up(key),
                        }
                    }
                }
            }
            // Window resized by user or OS
            Event::WindowEvent { event: WindowEvent::Resized(new_size), .. } => {
                let w = new_size.width.max(1);
                let h = new_size.height.max(1);
                pixels.resize_surface(w, h).unwrap();
            }
            // Window requests next frame redraw
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // Poll all pending gamepad events from gilrs
                if let Some(ref mut g) = gilrs {
                    while let Some(gilrs_event) = g.next_event() {
                        match gilrs_event.event {
                            EventType::ButtonPressed(button, _) => {
                                if let Some(key) = map_gamepad_button(button) {
                                    bus.joypad.key_down(key, &mut bus.if_flag);
                                }
                            }
                            EventType::ButtonReleased(button, _) => {
                                if let Some(key) = map_gamepad_button(button) {
                                    bus.joypad.key_up(key);
                                }
                            }
                            EventType::AxisChanged(axis, value, _) => {
                                match axis {
                                    Axis::LeftStickX | Axis::DPadX => {
                                        let new_right = value > STICK_DEADZONE;
                                        let new_left = value < -STICK_DEADZONE;

                                        if new_right != stick_right {
                                            stick_right = new_right;
                                            if new_right {
                                                bus.joypad.key_down(JoypadKey::Right, &mut bus.if_flag);
                                            } else {
                                                bus.joypad.key_up(JoypadKey::Right);
                                            }
                                        }
                                        if new_left != stick_left {
                                            stick_left = new_left;
                                            if new_left {
                                                bus.joypad.key_down(JoypadKey::Left, &mut bus.if_flag);
                                            } else {
                                                bus.joypad.key_up(JoypadKey::Left);
                                            }
                                        }
                                    }
                                    Axis::LeftStickY | Axis::DPadY => {
                                        let new_up = value > STICK_DEADZONE;
                                        let new_down = value < -STICK_DEADZONE;

                                        if new_up != stick_up {
                                            stick_up = new_up;
                                            if new_up {
                                                bus.joypad.key_down(JoypadKey::Up, &mut bus.if_flag);
                                            } else {
                                                bus.joypad.key_up(JoypadKey::Up);
                                            }
                                        }
                                        if new_down != stick_down {
                                            stick_down = new_down;
                                            if new_down {
                                                bus.joypad.key_down(JoypadKey::Down, &mut bus.if_flag);
                                            } else {
                                                bus.joypad.key_up(JoypadKey::Down);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            EventType::Connected => {
                                println!("Gamepad connected: {}", g.gamepad(gilrs_event.id).name());
                            }
                            EventType::Disconnected => {
                                println!("Gamepad disconnected: {}", g.gamepad(gilrs_event.id).name());
                            }
                            _ => {}
                        }
                    }
                }

                // Run CPU, Timer, and PPU cycles until an entire frame (144 scanlines) is complete
                while !bus.ppu.frame_ready {
                    let cycles = cpu.step(&mut bus);
                    bus.timer.step(cycles, &mut bus.if_flag);
                    bus.ppu.step(cycles, &mut bus.if_flag);
                }
                bus.ppu.frame_ready = false;

                let frame = pixels.frame_mut();

                // Clear the entire buffer to black, so what is not covered remains black
                for pixel in frame.chunks_exact_mut(4) {
                    pixel[0] = 0;
                    pixel[1] = 0;
                    pixel[2] = 0;
                    pixel[3] = 0xFF;
                }

                let frame = pixels.frame_mut();
                let src = &bus.ppu.framebuffer;
                
                frame.copy_from_slice(src);

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
            // Save state on any loop exit (e.g. fatal error or window close)
            Event::LoopExiting => {
                match bus.cartridge.battery_save(&sav_path) {
                    Ok(true) => println!("Saved game to {}", sav_path.display()),
                    Ok(false) => {},
                    Err(err) => eprintln!("Error saving game to {}: {}", sav_path.display(), err),
                }
            }
            _ => {}
        }
    }).unwrap();
}

/// Maps a physical keyboard key code from `winit` to a corresponding Game Boy [`JoypadKey`].
fn map_key_code(key_code: KeyCode) -> Option<JoypadKey> {
    match key_code {
        KeyCode::ArrowRight | KeyCode::KeyD => Some(JoypadKey::Right),
        KeyCode::ArrowLeft  | KeyCode::KeyA => Some(JoypadKey::Left),
        KeyCode::ArrowUp    | KeyCode::KeyW => Some(JoypadKey::Up),
        KeyCode::ArrowDown  | KeyCode::KeyS => Some(JoypadKey::Down),
        KeyCode::KeyZ       | KeyCode::KeyJ => Some(JoypadKey::A),
        KeyCode::KeyX       | KeyCode::KeyK => Some(JoypadKey::B),
        KeyCode::ShiftLeft  | KeyCode::ShiftRight | KeyCode::Backspace => Some(JoypadKey::Select),
        KeyCode::Enter      | KeyCode::Space => Some(JoypadKey::Start),
        _ => None,
    }
}

/// Maps a gamepad button from `gilrs` to a corresponding Game Boy [`JoypadKey`].
fn map_gamepad_button(button: Button) -> Option<JoypadKey> {
    match button {
        Button::DPadUp => Some(JoypadKey::Up),
        Button::DPadDown => Some(JoypadKey::Down),
        Button::DPadLeft => Some(JoypadKey::Left),
        Button::DPadRight => Some(JoypadKey::Right),
        Button::South | Button::North => Some(JoypadKey::A),
        Button::East | Button::West => Some(JoypadKey::B),
        Button::Start => Some(JoypadKey::Start),
        Button::Select => Some(JoypadKey::Select),
        _ => None,
    }
}
