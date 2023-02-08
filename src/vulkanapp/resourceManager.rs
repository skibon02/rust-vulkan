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
}

pub struct ResourceManager {
    pub resources: Vec<Resource>,
    pub host_access_policy: HostAccessPolicy,

    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
}

impl ResourceManager {
    pub fn new(instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: ash::Device, queue: vk::Queue, command_buffer: vk::CommandBuffer) -> Self {
        //query memory properties info
        let memory_properties = unsafe {instance.get_physical_device_memory_properties(physical_device)};

        let mut single_memory_type = memory_properties.memory_types.iter().enumerate().find(|(i, memory_type)| {
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


        Self {
            resources: Vec::new(),
            host_access_policy,

            physical_device,
            device,
            queue,
            command_buffer,
        }
    }

    pub fn create_buffer(&mut self, size: vk::DeviceSize, usage: vk::BufferUsageFlags) -> Resource {
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
            _ => {
                todo!("implement staging buffer case");
            }
        };

        let memory = unsafe {self.device.allocate_memory(&memory_allocate_info, None)}.unwrap();

        unsafe {self.device.bind_buffer_memory(buffer, memory, 0)}.unwrap();

        let res = Resource {
            buffer,
            memory,
        };
        self.resources.push(res);

        res
    }

    pub fn fill_buffer<T: Copy + Debug>(&mut self, resource: Resource, data: &[T]) {
        match self.host_access_policy {
            HostAccessPolicy::SingleBuffer(_) => {
                unsafe {
                    let mem_ptr = self.device.map_memory(resource.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).unwrap();
                    let mem_slice = std::slice::from_raw_parts_mut(mem_ptr as *mut T, data.len());
                    mem_slice.copy_from_slice(data);
                    self.device.unmap_memory(resource.memory);
                }
            },
            _ => {
                todo!("implement staging buffer case");
            }
        }
    }
}

