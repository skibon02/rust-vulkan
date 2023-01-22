mod vulkanapp;
use vulkanapp::VulkanApp;

use std::time::Instant;

use glfw;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const TITLE: &str = "Hello... Vulkan?";

fn main() {
    if cfg!(debug_assertions) {
        println!("Development build");
    }
    else {
        println!("Release build.");
    }

    let mut glfw = glfw::init(glfw::FAIL_ON_ERRORS).unwrap();
    glfw.window_hint(glfw::WindowHint::ClientApi(glfw::ClientApiHint::NoApi));
    if !glfw.vulkan_supported() {
        println!("Vulkan not supported");
        panic!("glfw: vulkan not supported");
    }
    
    let fullscreen = false;

    let (mut window, events) = match fullscreen {
        false => glfw.create_window(WIDTH, HEIGHT, TITLE, glfw::WindowMode::Windowed).unwrap(),
        true => glfw.with_primary_monitor(|glfw, m| {
            match m {
                Some(m) => {
                    let vidmode = m.get_video_mode().unwrap();
                    let (w,h) = (vidmode.width, vidmode.height);

                    println!("Monitor size: {}x{}", w, h);

                    glfw.create_window(w, h, TITLE, glfw::WindowMode::FullScreen(m))
                },
                None => {
                    println!("No monitor found");
                    glfw.create_window(WIDTH, HEIGHT, TITLE, glfw::WindowMode::Windowed)
                }
            }
        }).expect("Failed to create GLFW window")
    };
    let (screen_width, screen_height) =  window.get_framebuffer_size();


    println!("Screen size: {}x{}", screen_width, screen_height);
    
    window.set_key_polling(true);
    window.set_framebuffer_size_polling(true);

    let mut vulkan_app = VulkanApp::new(&glfw, &window);
    
    //set window resize callback
    let mut frames = 0;
    let start_time =  Instant::now();
    let mut prev_sec = 0;
    // let frame_seed = rand::random::<f32>();
    while !window.should_close() {
        {
            use glfw::WindowEvent as Event;
            use glfw::Key;
            use glfw::Action;
            glfw.poll_events();
            for (_, event) in glfw::flush_messages(&events) {
                match event {
                    Event::Key(Key::Escape, _, Action::Press, _) => {
                        window.set_should_close(true);
                    },
                    Event::FramebufferSize(w, h) => {
                        vulkan_app.framebuffer_resize(w as u32, h as u32, &window);
                    },
                    _ => {},
                }
            }
        }


        let timestamp = Instant::now().duration_since(start_time).as_secs_f32();

        //draw
        vulkan_app.draw_frame();

        //draw end
        //delay 1ms
        // std::thread::sleep(std::time::Duration::from_millis(1));

        frames += 1;
        let end = Instant::now().duration_since(start_time).as_secs();
        if end != prev_sec {
            println!("FPS: {}", frames);
            frames = 0;
            prev_sec = end;
        }

    }
}