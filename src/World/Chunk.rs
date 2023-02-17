mod Block;

// Chunks consist of 16x256x16 blocks.
// The blocks are stored in a 1D array.
pub struct Chunk {
    pub blocks: Vec<Block>,
    pub position: (i32, i32),
}