use crate::alias_tree::AliasTree;
use crate::control::Config;

pub struct ChunkTracker {
    chunk_count: usize,
    chunks: AliasTree<u8>,
}

const FLAG_UNPROCESSED: u8 = 2;
const FLAG_DIRTY: u8 = 1;

impl ChunkTracker {
    pub fn new(chunk_count: usize) -> Self {
        // let chunk_count = 
        //     devices.sectors / chunk_size
        //     + if devices.sectors % chunk_size {1} else {0};

        let chunks: AliasTree<u8> = AliasTree::new(chunk_count, FLAG_UNPROCESSED);

        ChunkTracker {
            chunk_count,
            chunks,
        }
    }

    pub fn get_chunk_count(&self) -> usize {
        self.chunk_count
    }

    // pub fn consume_dirty_queue(&mut self, change_log: &mut ChunkLog) {
    //     while let Some(index) = change_log.consume() {
    //         mark_chunk
    //     }
    // }

    // pub fn sector_to_chunk(&self, sector: usize) {
    //     sector / self.chunk_size;
    // }

    pub fn clear_chunk(&mut self, index: usize) {
        self.chunks.set(index, 0);
    }

    pub fn mark_chunk(&mut self, index: usize) {
        self.chunks.or_mask(index, FLAG_DIRTY);
    }

    #[allow(dead_code)]
    pub fn mark_chunks(&mut self, start: usize, end: usize) {
        let end = if end < self.chunk_count {
            end
        } else {
            self.chunk_count
        };
        for i in start..end {
            self.chunks.or_mask(i, FLAG_DIRTY);
        }
    }

    pub fn find_next(&self, start: usize) -> Option<usize> {
        self.chunks.find_next(|x|{*x!=0}, start)
    }

    pub fn summary_report(&self, config: &Config, height: usize) -> String {
        // BUG: If the chunk count isn't a multiple of 1<<height, the last chunk may not be representative.
        let factor: usize = 1 << height;
        let checks = (self.chunk_count-1)/factor+1;

        let mut diagram = String::with_capacity(checks * 7);
        let mut done = 0;

        for index in 0..checks {
            let flags = *self.chunks.get_aliased(index*factor, height);
            diagram.push_str(&config.diagram_cells[flags as usize]);
            if flags == 0 {
                done += 1;
            }
        }

        format!("\nChunk map ({} chunks per cell):\n{}{}\n\nProgess: {}%\n", factor, diagram, config.diagram_cells_reset, done * 100 / checks)
    }

    pub fn snapshot_level(&self, height: usize) -> Vec<u8> {
        let factor: usize = 1 << height;
        let checks = (self.chunk_count-1)/factor+1;
        let mut cells = Vec::with_capacity(checks);
        for index in 0..checks {
            cells.push(*self.chunks.get_aliased(index*factor, height));
        }
        cells
    }
}

pub fn calculate_display_detail(chunk_count: usize, limit: usize) -> usize {
    (
        if chunk_count <= limit {
            0
        } else {
            // Mathematically, the first ceil isn't necessary, but I'm
            // being (likely unnecessarily) paranoid about precision.
            (chunk_count as f64 / limit as f64).ceil().log2().ceil() as u64
        } as usize
    )
}
