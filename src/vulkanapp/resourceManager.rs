use std::fmt::Debug;

use ash::vk::{self, CommandBufferUsageFlags};

#[derive(Debug)]
pub enum HostAccessPolicy {
    UseStaging {
        host_memory_type: usize,
        device_memory_type: usize,
    },
    SingleBuffer(usize),
}

#[derive(Clone, Copy)]
pub struct BufferResource {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,
}

#[derive(Clone, Copy)]
pub struct ImageResource {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,

    pub width: u32,
    pub height: u32,
}

pub struct ResourceManager {
    pub host_access_policy: HostAccessPolicy,
    pub buffer_resources: Vec<BufferResource>,
    staging_buffer: Option<BufferResource>,

    pub image_resources: Vec<ImageResource>,

    device: ash::Device,
    queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
    transfer_completed_fence: vk::Fence,

    memory_types: Vec<vk::MemoryType>,
}

impl ResourceManager {
    pub fn new(instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: ash::Device, queue: vk::Queue, command_buffer: vk::CommandBuffer) -> Self {
        //query memory properties info
        let memory_properties = unsafe {instance.get_physical_device_memory_properties(physical_device)};

        let single_memory_type = memory_properties.memory_types.iter().enumerate().find(|(i, memory_type)| {
            if *i >= memory_properties.memory_type_count as usize {
                return false;
            }
            if memory_type.property_flags.contains( vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_COHERENT) {
                return true;
            }
            return false;
        });
        

        let host_access_policy = match single_memory_type {
            Some((i, _)) => HostAccessPolicy::SingleBuffer(i),
            None => {
                let host_visible_memory_type = memory_properties.memory_types.iter().enumerate().find(|(i, memory_type)| {

                    if *i >= memory_properties.memory_type_count as usize {
                        return false;
                    }
                    if memory_type.property_flags.contains( vk::MemoryPropertyFlags::HOST_COHERENT ) {
                        return true;
                    }
                    return false;
                });

                let device_memory_type = memory_properties.memory_types.iter().enumerate().find(|(i, memory_type)| {
                    if *i >= memory_properties.memory_type_count as usize {
                        return false;
                    }
                    if memory_type.property_flags.contains( vk::MemoryPropertyFlags::DEVICE_LOCAL ) {
                        return true;
                    }
                    return false;
                });
                
                match (host_visible_memory_type, device_memory_type) {
                    (Some((host_memory_type, _)), Some((device_memory_type, _))) => HostAccessPolicy::UseStaging {
                        host_memory_type,
                        device_memory_type,
                    },
                    _ => panic!("No suitable memory types found"),
                }
            }
        };

        println!("Host access policy: {:?}", host_access_policy);

        let fence = unsafe {device.create_fence(&vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED), None).unwrap()};

        Self {
            buffer_resources: Vec::new(),
            host_access_policy,

            image_resources: Vec::new(),

            device,
            queue,
            command_buffer,
            staging_buffer: None,
            transfer_completed_fence: fence,

            memory_types: memory_properties.memory_types.iter().map(|x| *x).collect(),
        }
    }

    pub fn create_buffer(&mut self, size: vk::DeviceSize, mut usage: vk::BufferUsageFlags) -> BufferResource {
        if let HostAccessPolicy::UseStaging { host_memory_type: _, device_memory_type: _ } = self.host_access_policy {
            usage |= vk::BufferUsageFlags::TRANSFER_DST;
        }
        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = unsafe {self.device.create_buffer(&buffer_create_info, None)}.unwrap();

        let memory_requirements = unsafe {self.device.get_buffer_memory_requirements(buffer)};

        let memory_allocate_info = match self.host_access_policy {
            HostAccessPolicy::SingleBuffer(memory_type) => {
                vk::MemoryAllocateInfo::builder()
                    .allocation_size(memory_requirements.size)
                    .memory_type_index(memory_type as u32)
            },
            HostAccessPolicy::UseStaging { host_memory_type: _, device_memory_type } => {
                vk::MemoryAllocateInfo::builder()
                    .allocation_size(memory_requirements.size)
                    .memory_type_index(device_memory_type as u32)
            }
        };

        let memory = unsafe {self.device.allocate_memory(&memory_allocate_info, None)}.unwrap();

        unsafe {self.device.bind_buffer_memory(buffer, memory, 0)}.unwrap();

        let res = BufferResource {
            buffer,
            memory,
            size,
        };
        self.buffer_resources.push(res);

        res
    }

    pub fn fill_buffer<T: Copy + Debug>(&mut self, resource: BufferResource, data: &[T]) {
        //size checktransfer_completed_fence
        let size = (data.len() * std::mem::size_of::<T>()) as vk::DeviceSize;
        assert!(size <= resource.size);


        unsafe {
            self.device.wait_for_fences(&[self.transfer_completed_fence], true, std::u64::MAX).unwrap();
            self.device.reset_fences(&[self.transfer_completed_fence]).unwrap();
            

            self.device.begin_command_buffer(self.command_buffer, 
                &vk::CommandBufferBeginInfo::builder()
                .flags(CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
        }
        match self.host_access_policy {
            HostAccessPolicy::SingleBuffer(_) => {
                //write to device_local
                unsafe {
                    let mem_ptr = self.device.map_memory(resource.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).unwrap();
                    let mem_slice = std::slice::from_raw_parts_mut(mem_ptr as *mut T, data.len());
                    mem_slice.copy_from_slice(data);
                    self.device.unmap_memory(resource.memory);
                }
            },
            HostAccessPolicy::UseStaging { host_memory_type, device_memory_type: _ } => {
                // write to stahing
                // transfer staging -> device_local
                //  transfer | vertex_input barrier
                let staging_buffer: BufferResource;
                
                if let Some(staging) = self.staging_buffer.take() {
                    staging_buffer = staging;
                } else {
                    let buffer_create_info = vk::BufferCreateInfo::builder()
                        .size(size)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE);
                    
                    let buffer = unsafe {self.device.create_buffer(&buffer_create_info, None)}.unwrap();

                    let memory_requirements = unsafe {self.device.get_buffer_memory_requirements(buffer)};

                    let memory_allocate_info = vk::MemoryAllocateInfo::builder()
                        .allocation_size(memory_requirements.size)
                        .memory_type_index(host_memory_type as u32);
                    
                    let memory = unsafe {self.device.allocate_memory(&memory_allocate_info, None)}.unwrap();

                    unsafe {self.device.bind_buffer_memory(buffer, memory, 0)}.unwrap();

                    staging_buffer = BufferResource {
                        buffer,
                        memory,
                        size,
                    };
                }
                unsafe {
                    let mem_ptr = self.device.map_memory(staging_buffer.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).unwrap();
                    let mem_slice = std::slice::from_raw_parts_mut(mem_ptr as *mut T, data.len());
                    mem_slice.copy_from_slice(data);
                    self.device.unmap_memory(staging_buffer.memory);
                }

                let copy_region = vk::BufferCopy::builder()
                    .size(size);

                unsafe {
                    self.device.cmd_copy_buffer(self.command_buffer, staging_buffer.buffer, resource.buffer, &[copy_region.build()]);
                    
                }

                //barrier transfer write to vertex shader read
                let buffer_memory_barrier = vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::VERTEX_ATTRIBUTE_READ)
                    .buffer(resource.buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE);
                
                unsafe {
                    self.device.cmd_pipeline_barrier(
                        self.command_buffer,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::VERTEX_INPUT,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[buffer_memory_barrier.build()],
                        &[],
                    );
                }
                self.staging_buffer = Some(staging_buffer);
            }
        }
        
        unsafe {
            self.device.end_command_buffer(self.command_buffer).unwrap();
            let submit_info = vk::SubmitInfo::builder()
                .command_buffers(&[self.command_buffer])
                .build();
            self.device.queue_submit(self.queue, &[submit_info], self.transfer_completed_fence).unwrap();
        }
    }
    pub fn cmd_barrier_after_vertex_buffer_use(&mut self, device: &ash::Device, command_buffer: vk::CommandBuffer, vertex_buffer: &BufferResource) {
        match self.host_access_policy {
            HostAccessPolicy::SingleBuffer(_) => {
                let buffer_memory_barrier = vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::VERTEX_ATTRIBUTE_READ)
                    .dst_access_mask(vk::AccessFlags::HOST_WRITE)
                    .buffer(vertex_buffer.buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE);
                
                unsafe {
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::VERTEX_INPUT,
                        vk::PipelineStageFlags::HOST,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[buffer_memory_barrier.build()],
                        &[],
                    );
                }
            },
            HostAccessPolicy::UseStaging { host_memory_type: _, device_memory_type: _ } => {
                let buffer_memory_barrier = vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::VERTEX_ATTRIBUTE_READ)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .buffer(vertex_buffer.buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE);
                
                unsafe {
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::VERTEX_INPUT,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[buffer_memory_barrier.build()],
                        &[],
                    );
                }
            }
        }
    }


    pub fn create_image(&mut self, width: u32, height: u32, format: vk::Format, tiling: vk::ImageTiling, usage: vk::ImageUsageFlags) -> ImageResource {
        let image_create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(tiling)
            .usage(usage | vk::ImageUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        
        let image = unsafe {self.device.create_image(&image_create_info, None)}.unwrap();

        let memory_requirements = unsafe {self.device.get_image_memory_requirements(image)};

        let memory_type_device  = self.memory_types.iter().enumerate().position(|(i, memory_type)| {
            memory_requirements.memory_type_bits & (1 << i) != 0 && memory_type.property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        }).unwrap();

        let memory_allocate_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_device as u32);
        
        let memory = unsafe {self.device.allocate_memory(&memory_allocate_info, None)}.unwrap();

        unsafe {self.device.bind_image_memory(image, memory, 0)}.unwrap();

        ImageResource {
            image,
            memory,
            size: memory_requirements.size,
            width,
            height
        }
    }

    // TODO: save buffer or free it
    pub fn fill_image(&mut self, imageResource: ImageResource, data: &[u8]) {
        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(data.len() as u64)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        
        let buffer = unsafe {self.device.create_buffer(&buffer_create_info, None)}.unwrap();

        let memory_requirements = unsafe {self.device.get_buffer_memory_requirements(buffer)};

        let memory_type_host = self.memory_types.iter().enumerate().position(|(i, memory_type)| {
            memory_requirements.memory_type_bits & (1 << i) != 0 && memory_type.property_flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
        }).unwrap();

        let memory_allocate_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_host as u32);
        
        let memory = unsafe {self.device.allocate_memory(&memory_allocate_info, None)}.unwrap();

        unsafe {self.device.bind_buffer_memory(buffer, memory, 0)}.unwrap();

        unsafe {
            let mem_ptr = self.device.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).unwrap();
            let mem_slice = std::slice::from_raw_parts_mut(mem_ptr as *mut u8, data.len());
            mem_slice.copy_from_slice(data);
            self.device.unmap_memory(memory);
        }

        let copy_region = vk::BufferImageCopy::builder()
            .image_subresource(vk::ImageSubresourceLayers::builder()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1)
                .build())
            .image_extent(vk::Extent3D {
                width: imageResource.width,
                height: imageResource.height,
                depth: 1,
            });
        
        unsafe {
            self.device.begin_command_buffer(self.command_buffer, &vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
            
            // transition image layout from undefined to transfer destination
            let image_memory_barrier = vk::ImageMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .image(imageResource.image)
                .subresource_range(vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build());

            self.device.cmd_pipeline_barrier(self.command_buffer, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[image_memory_barrier.build()]);
            
            self.device.cmd_copy_buffer_to_image(self.command_buffer, buffer, imageResource.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[copy_region.build()]);
            
            // transition image layout from transfer destination to shader read
            let image_memory_barrier = vk::ImageMemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image(imageResource.image)
                .subresource_range(vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build());

            self.device.cmd_pipeline_barrier(self.command_buffer, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[image_memory_barrier.build()]);
            
            self.device.end_command_buffer(self.command_buffer).unwrap();

            let submit_info = vk::SubmitInfo::builder()
                .command_buffers(&[self.command_buffer]).build();

            self.device.queue_submit(self.queue, &[submit_info], vk::Fence::null()).unwrap();

            self.device.queue_wait_idle(self.queue).unwrap();
        }
    }

    pub fn create_image_view(&self, image: vk::Image, format: vk::Format, aspect_flags: vk::ImageAspectFlags) -> vk::ImageView {
        let image_view_create_info = vk::ImageViewCreateInfo::builder()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(vk::ImageSubresourceRange::builder()
                .aspect_mask(aspect_flags)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1)
                .build());
        
        unsafe {self.device.create_image_view(&image_view_create_info, None)}.unwrap()
    }

    pub fn create_sampler(&self) -> vk::Sampler {
        let sampler_create_info = vk::SamplerCreateInfo::builder()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::REPEAT)
            .address_mode_v(vk::SamplerAddressMode::REPEAT)
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            .anisotropy_enable(false)
            .max_anisotropy(16.0)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
            .min_lod(0.0)
            .max_lod(0.0)
            .mip_lod_bias(0.0);
        
        unsafe {self.device.create_sampler(&sampler_create_info, None)}.unwrap()
    }
}

