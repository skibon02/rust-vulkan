use std::ffi::c_void;

use ash::{vk::{self, Handle, SurfaceKHR}, Entry, extensions};
use std::ptr;

struct SyncObjects {
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    in_flight_fence: vk::Fence,
}
struct SwapchainDependentResources {
    swapchain_loader: ash::extensions::khr::Swapchain,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_format: vk::Format,
    swapchain_extent: vk::Extent2D,
    swapchain_imageviews: Vec<vk::ImageView>,
    swapchain_framebuffers: Vec<vk::Framebuffer>,


    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    graphics_pipeline: vk::Pipeline,
}

pub struct VulkanApp {
    // vulkan stuff
    entry: ash::Entry,
    instance: ash::Instance,
    surface: vk::SurfaceKHR,
    debug_utils_loader: Option<ash::extensions::ext::DebugUtils>,
    debug_messenger: Option<vk::DebugUtilsMessengerEXT>,

    physical_device: vk::PhysicalDevice,
    device: ash::Device,

    queue: vk::Queue,

    swapchain_dependent_resources: Option<SwapchainDependentResources>,

    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,

    sync_objects: SyncObjects,
}

impl VulkanApp {
    pub fn new(glfw: &glfw::Glfw, window: &glfw::Window) -> VulkanApp {

        let required_extensions = glfw.get_required_instance_extensions().unwrap().iter()
            .map(|s| s.clone()+"\0")
            .collect::<Vec<String>>();

        let mut instance_extensions = Vec::new();
        instance_extensions.push(vk::KhrGetPhysicalDeviceProperties2Fn::name().as_ptr());
        for i in &required_extensions {
            instance_extensions.push(i.as_ptr() as *const i8);
        }

        let mut validation_layers = Vec::new();
        if cfg!(debug_assertions) {
            instance_extensions.push(vk::ExtDebugUtilsFn::name().as_ptr());
            validation_layers.push("VK_LAYER_KHRONOS_validation\0".as_ptr() as *const i8);
        }


        let entry = Entry::linked();
        //check if extensions are supported
        let mut supported = true;
        let available_extensions = entry.enumerate_instance_extension_properties(None).unwrap();
        for i in &instance_extensions {
            let requested_ext_name = unsafe { std::ffi::CStr::from_ptr(*i) };
            let mut found = false;
            for j in &available_extensions {
                let available_ext_name = unsafe { std::ffi::CStr::from_ptr(j.extension_name.as_ptr()) };
                if requested_ext_name == available_ext_name {
                    found = true;
                    break;
                }
            }
            if !found {
                println!("Extension {} is not supported", requested_ext_name.to_str().unwrap());
                supported = false;
            }
        }
        if !supported {
            panic!("Not all extensions are supported");
        }

        //check if validation layers are supported
        let available_layers = entry.enumerate_instance_layer_properties().unwrap();
        for i in &validation_layers {
            let requested_layer_name = unsafe { std::ffi::CStr::from_ptr(*i) };
            let mut found = false;
            for j in &available_layers {
                let available_layer_name = unsafe { std::ffi::CStr::from_ptr(j.layer_name.as_ptr()) };
                if requested_layer_name == available_layer_name {
                    found = true;
                    break;
                }
            }
            if !found {
                println!("Layer {} is not supported", requested_layer_name.to_str().unwrap());
                supported = false;
            }
        }
        if !supported {
            panic!("Not all layers are supported");
        }


        let app_info = vk::ApplicationInfo {
            api_version: vk::API_VERSION_1_3,
            p_application_name: "Hello Triangle\0".as_ptr() as *const i8,
            application_version: vk::make_api_version(0, 1, 0, 0),
            p_engine_name: "No Engine\0".as_ptr() as *const i8,
            ..Default::default()
        };
        
        let mut create_info = vk::InstanceCreateInfo {
            p_application_info: &app_info,
            pp_enabled_extension_names: instance_extensions.as_ptr(),
            enabled_extension_count: instance_extensions.len().try_into().unwrap(),
            pp_enabled_layer_names: validation_layers.as_ptr(),
            enabled_layer_count: validation_layers.len().try_into().unwrap(),
            ..Default::default()
        };
        let debug_messanger_create_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
            .message_type(vk::DebugUtilsMessageTypeFlagsEXT::GENERAL | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE)
            .pfn_user_callback(Some(vulkan_debug_callback))
            .build();
        if cfg!(debug_assertions) {
            println!("Validation layers enabled");
            create_info.p_next = &debug_messanger_create_info as *const _ as *const c_void;
        }
        let instance_res = unsafe { entry.create_instance(&create_info, None) };

        let instance: ash::Instance;
        match instance_res {
            Ok(i) => {
                instance = i;
                println!("Instance created");

            },
            Err(e) => {
                println!("Instance creation failed: {:?}", e);
                panic!("Instance creation failed");
            }
        }
        // Instance is created
        let debug_utils_loader: Option<ash::extensions::ext::DebugUtils>;
        let debug_messenger: Option<vk::DebugUtilsMessengerEXT>;
        if cfg!(debug_assertions) {
            let debug_utils_loader_ins = extensions::ext::DebugUtils::new(&entry, &instance);
            debug_messenger = Some(unsafe {debug_utils_loader_ins.create_debug_utils_messenger(&debug_messanger_create_info, None).unwrap()});
            debug_utils_loader = Some(debug_utils_loader_ins);
        }
        else {
            debug_utils_loader = None;
            debug_messenger = None;
        }
        
        let physical_devices = unsafe { instance.enumerate_physical_devices().unwrap() };

        let physical_device = *physical_devices.iter().find(|&d| {
            let properties = unsafe { instance.get_physical_device_properties(*d) };
            properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
        }).or_else(|| {
            physical_devices.iter().find(|&d| {
                let properties = unsafe { instance.get_physical_device_properties(*d) };
                properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
            })
        }).or_else(|| {
            physical_devices.iter().find(|&d| {
                let properties = unsafe { instance.get_physical_device_properties(*d) };
                properties.device_type == vk::PhysicalDeviceType::CPU
            })
        }).unwrap_or_else(|| {
            panic!("No avaliable physical device found");
        });
        
        //select chosen physical device
        let dev_name_array = unsafe { instance.get_physical_device_properties(physical_device).device_name };
        let dev_name = unsafe {std::ffi::CStr::from_ptr(dev_name_array.as_ptr())};
        println!("Chosen device: {}", dev_name.to_str().unwrap());


        let queue_family_properties = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let queue_family_index = queue_family_properties.iter().enumerate().find(|(_, p)| {
            p.queue_flags.contains(vk::QueueFlags::GRAPHICS) 
        }).map(|(i, _)| i as u32).unwrap();

        let mut surface : u64 = 0;
        window.create_window_surface(instance.handle().as_raw() as usize, 0 as *const _, &mut surface);
        let surface = vk::SurfaceKHR::from_raw(surface);

        let presentation_support = glfw.get_physical_device_presentation_support_raw(instance.handle().as_raw() as usize, physical_device.as_raw() as usize, queue_family_index);
        if !presentation_support {
            panic!("Presentation not supported");
        }

        let mut device_extensions = Vec::new();
        device_extensions.push(vk::KhrSwapchainFn::name().as_ptr());

        let queue_create_infos = [vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])
            .build()];
        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions)
            .enabled_layer_names(&validation_layers);

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None).unwrap() };
        

        // Device and Surface are created

        
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        
        let swapchain_dependent_stuff =  VulkanApp::create_swapchain_dependent_resources(window, &entry, &instance, &physical_device, surface, &device, None); // swapchain and all dependent resources are created

        let command_pool = unsafe { device.create_command_pool(&vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .build(), None).unwrap() };
        
        let command_buffers = unsafe { device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::builder()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(swapchain_dependent_stuff.swapchain_images.len() as u32)
            .build()).unwrap() };

        let image_available_semaphore = 
            unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() };
        let render_finished_semaphore = 
            unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).unwrap() };
        let in_flight_fence = 
            unsafe { device.create_fence(&vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED).build(), None).unwrap() };

        return VulkanApp {
            entry,
            instance,
            debug_utils_loader,
            debug_messenger,
            physical_device,
            device,
            surface,
            queue,
            swapchain_dependent_resources: Some(swapchain_dependent_stuff),
            command_pool,
            command_buffers,
            sync_objects: SyncObjects {
                image_available_semaphore,
                render_finished_semaphore,
                in_flight_fence,
            }
        };
    }

    pub fn draw_frame(&mut self) -> bool {

        let swapchain = self.swapchain_dependent_resources.as_ref().unwrap();
        let device = &self.device;
        // 1) wait for image available
        let (image_index, _is_sub_optimal) = unsafe {
            device.wait_for_fences(&[self.sync_objects.in_flight_fence], true, std::u64::MAX).expect("Failed to wait for Fence!");

            device.reset_fences(&[self.sync_objects.in_flight_fence]).expect("Failed to reset Fence!");

            swapchain.swapchain_loader
                .acquire_next_image(
                    swapchain.swapchain,
                    std::u64::MAX,
                    self.sync_objects.image_available_semaphore,
                    vk::Fence::null(),
                )
                .expect("Failed to acquire next image.")
        };
        if _is_sub_optimal {
            println!("acquire_next_image: Suboptimal swapchain image");
        }

        // 2.0) record command buffer
        let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE)
            .build();

        unsafe {
            let reset_res = device
                .reset_command_buffer(self.command_buffers[image_index as usize], vk::CommandBufferResetFlags::empty());
            match reset_res {
                Ok(_) => {},
                Err(e) => {
                    panic!("Failed to reset command buffer: {}", e.to_string());
                }
            }


            let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
                .render_pass(swapchain.render_pass)
                .framebuffer(swapchain.swapchain_framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: swapchain.swapchain_extent,
                })
                .clear_values(&[vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.8, 0.4, 0.7, 1.0],
                    },
                }])
                .build();


            device
                .begin_command_buffer(self.command_buffers[image_index as usize], &command_buffer_begin_info)
                .expect("Failed to begin recording command buffer!");
            device
                .cmd_begin_render_pass(self.command_buffers[image_index as usize], &render_pass_begin_info, vk::SubpassContents::INLINE);

            device
                .cmd_bind_pipeline(self.command_buffers[image_index as usize], vk::PipelineBindPoint::GRAPHICS, swapchain.graphics_pipeline);
            device
                .cmd_draw(self.command_buffers[image_index as usize], 3, 1, 0, 0);

            device
                .cmd_end_render_pass(self.command_buffers[image_index as usize]);
            let end_cb_res = device
                .end_command_buffer(self.command_buffers[image_index as usize]);
            match end_cb_res {
                Ok(_) => {},
                Err(e) => {
                    panic!("Failed to end recording command buffer: {}", e.to_string());
                }
            }
        }


        // 2) queue submit
        let submit_infos = [vk::SubmitInfo {
            s_type: vk::StructureType::SUBMIT_INFO,
            p_next: ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.sync_objects.image_available_semaphore,
            p_wait_dst_stage_mask: &vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            command_buffer_count: 1,
            p_command_buffers: &self.command_buffers[image_index as usize],
            signal_semaphore_count: 1,
            p_signal_semaphores: &self.sync_objects.render_finished_semaphore,
        }];

        unsafe {
            device
                .reset_fences(&[self.sync_objects.in_flight_fence])
                .expect("Failed to reset Fence!");

            device
                .queue_submit(
                    self.queue,
                    &submit_infos,
                    self.sync_objects.in_flight_fence,
                )
                .expect("Failed to execute queue submit.");
        }

        // 3) present
        let swapchains = [swapchain.swapchain];

        let present_info = vk::PresentInfoKHR {
            s_type: vk::StructureType::PRESENT_INFO_KHR,
            p_next: ptr::null(),
            wait_semaphore_count: 1,
            p_wait_semaphores: &self.sync_objects.render_finished_semaphore,
            swapchain_count: 1,
            p_swapchains: swapchains.as_ptr(),
            p_image_indices: &image_index,
            p_results: ptr::null_mut(),
        };

        unsafe {
            match swapchain.swapchain_loader.queue_present(self.queue, &present_info) {
                Ok(is_suboptimal) if is_suboptimal == true  => {
                    println!("queue_present: Suboptimal swapchain image");
                },
                Err(e) => {
                    println!("queue_present: {}", e.to_string());
                }
                Ok(_) => {}
            }
        }
        return true;
    }
    
    fn create_swapchain_dependent_resources(window: &glfw::Window, entry: &ash::Entry, instance: &ash::Instance, physical_device: &vk::PhysicalDevice, surface: SurfaceKHR, device: &ash::Device, old_swapchain: Option<vk::SwapchainKHR>) -> SwapchainDependentResources {

        //query swapchain support
        let surface_loader = extensions::khr::Surface::new(entry, instance);
        let surface_capabilities = unsafe { surface_loader.get_physical_device_surface_capabilities(*physical_device, surface).unwrap() };
        let surface_formats = unsafe { surface_loader.get_physical_device_surface_formats(*physical_device, surface).unwrap() };
        let surface_present_modes = unsafe { surface_loader.get_physical_device_surface_present_modes(*physical_device, surface).unwrap() };

        //prefer VK_FORMAT_B8G8R8A8_UNORM and VK_COLOR_SPACE_SRGB_NONLINEAR_KHR
        let surface_format = surface_formats.iter().find(|f| {
            f.format == vk::Format::B8G8R8A8_UNORM && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        }).unwrap_or_else(|| {
            surface_formats.first().unwrap()
        });
        //prefer MAILBOX then IMMEDIATE or default FIFO
        let present_mode = surface_present_modes.iter().find(|m| {
            **m == vk::PresentModeKHR::MAILBOX
        }).unwrap_or_else(|| {
            surface_present_modes.iter().find(|m| {
                **m == vk::PresentModeKHR::IMMEDIATE
            }).unwrap_or_else(|| {
                surface_present_modes.first().unwrap()
            })
        });
        println!("Present mode: {:?}", present_mode);

        let extent = window.get_framebuffer_size();

        let swapchain_extent = if surface_capabilities.current_extent.width != u32::MAX {
            surface_capabilities.current_extent
        } else {
            let mut actual_extent = vk::Extent2D::builder()
                .width(extent.0 as u32)
                .height(extent.1 as u32)
                .build();
            actual_extent.width = actual_extent.width.max(surface_capabilities.min_image_extent.width).min(surface_capabilities.max_image_extent.width);
            actual_extent.height = actual_extent.height.max(surface_capabilities.min_image_extent.height).min(surface_capabilities.max_image_extent.height);
            actual_extent
        };

        let image_count = surface_capabilities.min_image_count;

        let swapchain_loader = extensions::khr::Swapchain::new(&instance, device);
        let mut swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(swapchain_extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(*present_mode)
            .clipped(true);

        if let Some(old_swapchain) = old_swapchain {
            swapchain_create_info = swapchain_create_info.old_swapchain(old_swapchain);
        }
        let swapchain_create_info = swapchain_create_info.build();
        
        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None).unwrap() };
        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain).unwrap() };

        let swapchain_imageviews = swapchain_images.iter().map(|image| {
            let image_view_create_info = vk::ImageViewCreateInfo::builder()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .components(vk::ComponentMapping::builder()
                    .r(vk::ComponentSwizzle::IDENTITY)
                    .g(vk::ComponentSwizzle::IDENTITY)
                    .b(vk::ComponentSwizzle::IDENTITY)
                    .a(vk::ComponentSwizzle::IDENTITY)
                    .build())
                .subresource_range(vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build())
                .build();
            unsafe { device.create_image_view(&image_view_create_info, None).unwrap() }
        }).collect::<Vec<_>>();

        // swapchain and image views are created

        let render_pass = {
            let color_attachments = [vk::AttachmentDescription::builder()
                .format(surface_format.format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .build()];
            let color_attachment_refs = [vk::AttachmentReference::builder()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .build()];
            let subpasses = [vk::SubpassDescription::builder()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_attachment_refs)
                .build()];
            let render_pass_create_info = vk::RenderPassCreateInfo::builder()
                .attachments(&color_attachments)
                .subpasses(&subpasses)
                .build();
            unsafe { device.create_render_pass(&render_pass_create_info, None).unwrap() }
        };

        let framebuffers = swapchain_imageviews.iter().map(|image_view| {
            let framebuffer_create_info = vk::FramebufferCreateInfo::builder()
                .render_pass(render_pass)
                .attachments(&[*image_view])
                .width(swapchain_extent.width)
                .height(swapchain_extent.height)
                .layers(1)
                .build();
            unsafe { device.create_framebuffer(&framebuffer_create_info, None).unwrap() }
        }).collect::<Vec<_>>();

        //render pass and framebuffers are created

        //load shaders from file
        let vertex_shader_code = std::fs::read("shaders/vert.spv").unwrap();
        let fragment_shader_code = std::fs::read("shaders/frag.spv").unwrap();
        
        let mut shader_module_create_info = vk::ShaderModuleCreateInfo {
            s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO,
            p_next: std::ptr::null(),
            flags: vk::ShaderModuleCreateFlags::empty(),
            code_size: vertex_shader_code.len(),
            p_code: vertex_shader_code.as_ptr() as *const u32,
        };
        let vertex_shader_module = unsafe { device.create_shader_module(&shader_module_create_info, None).unwrap() };

        shader_module_create_info.code_size = fragment_shader_code.len();
        shader_module_create_info.p_code = fragment_shader_code.as_ptr() as *const u32;
        let fragment_shader_module = unsafe { device.create_shader_module(&shader_module_create_info, None).unwrap() };

        let vertex_shader_stage_create_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex_shader_module)
            .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
            .build();
        let fragment_shader_stage_create_info = vk::PipelineShaderStageCreateInfo::builder()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment_shader_module)
            .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
            .build();

        let shader_stages = [vertex_shader_stage_create_info, fragment_shader_stage_create_info];

        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::builder()
            .vertex_binding_descriptions(&[])
            .vertex_attribute_descriptions(&[])
            .build();

        let dynamic_state_create_info = vk::PipelineDynamicStateCreateInfo::builder()
            .dynamic_states(&[])
            .build();

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::builder()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false)
            .build();

        let viewports = [vk::Viewport::builder()
            .x(0.0)
            .y(0.0)
            .width(swapchain_extent.width as f32)
            .height(swapchain_extent.height as f32)
            .min_depth(0.0)
            .max_depth(1.0)
            .build()];

        let scissors = [vk::Rect2D::builder()
            .offset(vk::Offset2D::builder().x(0).y(0).build())
            .extent(swapchain_extent)
            .build()];
        
        let viewport_state = vk::PipelineViewportStateCreateInfo::builder()
            .viewports(&viewports)
            .scissors(&scissors)
            .build();

        let rasterizer = vk::PipelineRasterizationStateCreateInfo::builder()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::BACK)
            .front_face(vk::FrontFace::CLOCKWISE)
            .depth_bias_enable(false)
            .build();

        let multisampling = vk::PipelineMultisampleStateCreateInfo::builder()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1)
            .build();

        let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::builder()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false)
            .build()];

        let color_blending = vk::PipelineColorBlendStateCreateInfo::builder()
            .logic_op_enable(false)
            .logic_op(vk::LogicOp::COPY)
            .attachments(&color_blend_attachments)
            .build();

        let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&[])
            .push_constant_ranges(&[])
            .build();

        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_create_info, None).unwrap() };

        let graphics_pipeline_create_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_create_info)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0)
            .build();

        let graphics_pipelines = unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[graphics_pipeline_create_info], None).unwrap() };

        unsafe {
            device.destroy_shader_module(vertex_shader_module, None);
            device.destroy_shader_module(fragment_shader_module, None);
        }

        
        return SwapchainDependentResources { 
            render_pass,
            graphics_pipeline: graphics_pipelines[0],
            pipeline_layout,

            swapchain: swapchain,
            swapchain_images: swapchain_images,
            swapchain_imageviews,
            swapchain_format: surface_format.format,
            swapchain_extent,
            swapchain_framebuffers: framebuffers,
            swapchain_loader: swapchain_loader,
        };     
    }
    fn recreate_swapchain(&mut self, window: &glfw::Window) {
        let (mut w, mut h) = window.get_framebuffer_size();
        while w == 0 || h == 0 {
            (w, h) = window.get_framebuffer_size();
        }

        unsafe { self.device.device_wait_idle().expect("Failed to wait for device idle!"); }

        //free resources
        match self.swapchain_dependent_resources {
            Some(ref mut swapchain_dependent_resources) => {
                //free resources

                for framebuffer in swapchain_dependent_resources.swapchain_framebuffers.iter() {
                    unsafe { self.device.destroy_framebuffer(*framebuffer, None); }
                }

                unsafe { self.device.destroy_pipeline(swapchain_dependent_resources.graphics_pipeline, None); }
                unsafe { self.device.destroy_pipeline_layout(swapchain_dependent_resources.pipeline_layout, None); }
                unsafe { self.device.destroy_render_pass(swapchain_dependent_resources.render_pass, None); }

                for imageview in swapchain_dependent_resources.swapchain_imageviews.iter() {
                    unsafe { self.device.destroy_image_view(*imageview, None); }
                }

                let old_swapchain = swapchain_dependent_resources.swapchain;

                self.swapchain_dependent_resources = Some(VulkanApp::create_swapchain_dependent_resources(
                    &window,
                    &self.entry,
                    &self.instance,
                    &self.physical_device,
                    self.surface,
                    &self.device,
                    Some(old_swapchain),
                ));

                unsafe { self.swapchain_dependent_resources.as_ref().unwrap().swapchain_loader.destroy_swapchain(old_swapchain, None); }



            },
            None => {
                println!("No swapchain dependent resources to free");
            }
        }

    }
    pub fn framebuffer_resize(&mut self, width: u32, height: u32, window: &glfw::Window) {
        println!("Framebuffer resized to {}x{}", width, height);
        self.recreate_swapchain(window);
    }
}


unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let callback_data = unsafe { &*p_callback_data };
    let msg = unsafe { std::ffi::CStr::from_ptr(callback_data.p_message) };
    println!(
        "validation layer: {:?} {:?}: {}",
        message_severity, message_type, msg.to_str().unwrap()
    );
    vk::FALSE
}