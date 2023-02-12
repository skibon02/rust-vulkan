use std::fmt::Debug;

use ash::vk;

#[derive(Debug)]
pub enum HostAccessPolicy {
    UseStaging {
        host_memory_type: usize,
        device_memory_type: usize,
    },
    SingleBuffer(usize),
}

#[derive(Clone, Copy)]
pub struct Resource {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,
}

pub struct ResourceManager {
    pub resources: Vec<Resource>,
    pub host_access_policy: HostAccessPolicy,
    stagingBuffer: Option<Resource>,

    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
    transfer_completed_fence: Option<vk::Fence>,
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

        Self {
            resources: Vec::new(),
            host_access_policy,

            physical_device,
            device,
            queue,
            command_buffer,
            stagingBuffer: None,
            transfer_completed_fence: None,
        }
    }

    pub fn create_buffer(&mut self, size: vk::DeviceSize, mut usage: vk::BufferUsageFlags) -> Resource {
        if let HostAccessPolicy::UseStaging { host_memory_type: _, device_memory_type: _ } = self.host_access_policy {
            usage |= vk::BufferUsageFlags::TRANSFER_DST;
            let fence = unsafe {self.device.create_fence(&vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED), None).unwrap()};
            self.transfer_completed_fence = Some(fence);
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

        let res = Resource {
            buffer,
            memory,
            size,
        };
        self.resources.push(res);

        res
    }

    pub fn fill_buffer<T: Copy + Debug>(&mut self, resource: Resource, data: &[T]) {
        //size check
        let size = (data.len() * std::mem::size_of::<T>()) as vk::DeviceSize;
        assert!(size <= resource.size);

        match self.host_access_policy {
            HostAccessPolicy::SingleBuffer(_) => {
                unsafe {
                    let mem_ptr = self.device.map_memory(resource.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).unwrap();
                    let mem_slice = std::slice::from_raw_parts_mut(mem_ptr as *mut T, data.len());
                    mem_slice.copy_from_slice(data);
                    self.device.unmap_memory(resource.memory);
                }
            },
            HostAccessPolicy::UseStaging { host_memory_type, device_memory_type: _ } => {
                unsafe {
                    self.device.wait_for_fences(&[self.transfer_completed_fence.unwrap()], true, std::u64::MAX).unwrap();
                    self.device.reset_fences(&[self.transfer_completed_fence.unwrap()]).unwrap();
                }
                
                let staging_buffer: Resource;
                
                if let Some(staging) = self.stagingBuffer.take() {
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

                    staging_buffer = Resource {
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
                    .size((data.len() * std::mem::size_of::<T>()) as vk::DeviceSize);


                unsafe {
                    self.device.begin_command_buffer(self.command_buffer, 
                        &vk::CommandBufferBeginInfo::builder()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
                }

                let buffer_memory_barrier = vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::HOST_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .buffer(staging_buffer.buffer)
                    .offset(0)
                    .size(vk::WHOLE_SIZE);


                unsafe {
                    self.device.cmd_pipeline_barrier(
                        self.command_buffer,
                        vk::PipelineStageFlags::HOST,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[buffer_memory_barrier.build()],
                        &[],
                    );
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

                    self.device.end_command_buffer(self.command_buffer).unwrap();
                }

                unsafe {
                    let submit_info = vk::SubmitInfo::builder()
                        .command_buffers(&[self.command_buffer])
                        .build();
                    self.device.queue_submit(self.queue, &[submit_info], self.transfer_completed_fence.unwrap()).unwrap();
                }
                self.stagingBuffer = Some(staging_buffer);
            }
        }
    }
    pub fn cmd_barrier_after_vertex_buffer_use(&mut self, device: &ash::Device, command_buffer: vk::CommandBuffer, vertex_buffer: &Resource) {
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
}

