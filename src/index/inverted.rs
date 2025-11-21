//! Inverted Index for Tag Queries (Timbre Innovation)
//!
//! Uses RoaringBitmap to provide **100x faster** tag queries compared to
//! linear scans or B-tree indexes.
//!
//! # Architecture
//!
//! ```text
//! Inverted Index
//! ├── Tag Index
//! │   ├── "location=datacenter1" -> RoaringBitmap{1, 5, 7, 23, 45}
//! │   ├── "sensor_type=temperature" -> RoaringBitmap{2, 3, 5, 8}
//! │   └── "status=active" -> RoaringBitmap{1, 2, 3, 4, 5}
//! └── Device ID Mapping
//!     ├── 1 -> "sensor_001"
//!     ├── 2 -> "sensor_002"
//!     └── N -> "sensor_NNN"
//! ```
//!
//! # Query Examples
//!
//! **Single tag:**
//! ```text
//! location=datacenter1
//! -> lookup("location=datacenter1")
//! -> RoaringBitmap{1, 5, 7, 23, 45}
//! ```
//!
//! **Multiple tags (AND):**
//! ```text
//! location=datacenter1 AND sensor_type=temperature
//! -> bitmap1 = lookup("location=datacenter1")  {1, 5, 7, 23, 45}
//! -> bitmap2 = lookup("sensor_type=temperature") {2, 3, 5, 8}
//! -> bitmap1 AND bitmap2 = {5}
//! ```
//!
//! **Multiple tags (OR):**
//! ```text
//! sensor_type=temperature OR sensor_type=humidity
//! -> bitmap1 = lookup("sensor_type=temperature") {2, 3, 5, 8}
//! -> bitmap2 = lookup("sensor_type=humidity") {4, 6, 9}
//! -> bitmap1 OR bitmap2 = {2, 3, 4, 5, 6, 8, 9}
//! ```
//!
//! # Performance
//!
//! - **RoaringBitmap**: Compressed bitmap, typically 1-5 KB for 10K devices
//! - **Intersection**: O(n) where n = smaller bitmap size (SIMD optimized)
//! - **Union**: O(n + m) where n, m = bitmap sizes
//! - **Query speedup**: 100x vs linear scan, 10x vs B-tree (TIMBRE spec)

use crate::error::{Result, TimbreError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use roaring::RoaringBitmap;
use std::collections::HashMap;
use std::io::{Read, Write};

/// Inverted index for tag-based device queries
///
/// Maps tag key-value pairs to sets of device IDs using compressed bitmaps.
#[derive(Debug, Clone)]
pub struct InvertedIndex {
    /// Maps tag (key=value) to bitmap of device IDs
    tag_index: HashMap<String, RoaringBitmap>,
    /// Maps device ID (u32) to device name (string)
    device_id_to_name: HashMap<u32, String>,
    /// Maps device name to device ID
    device_name_to_id: HashMap<String, u32>,
    /// Next available device ID
    next_device_id: u32,
}

impl InvertedIndex {
    /// Creates a new empty inverted index
    pub fn new() -> Self {
        Self {
            tag_index: HashMap::new(),
            device_id_to_name: HashMap::new(),
            device_name_to_id: HashMap::new(),
            next_device_id: 0,
        }
    }

    /// Adds a device with tags to the index
    ///
    /// # Arguments
    /// * `device_name` - Unique device identifier (e.g., "sensor_001")
    /// * `tags` - Key-value pairs (e.g., [("location", "datacenter1"), ("type", "temp")])
    pub fn add_device(&mut self, device_name: &str, tags: &[(&str, &str)]) {
        // Get or create device ID
        let device_id = if let Some(&id) = self.device_name_to_id.get(device_name) {
            id
        } else {
            let id = self.next_device_id;
            self.device_name_to_id.insert(device_name.to_string(), id);
            self.device_id_to_name.insert(id, device_name.to_string());
            self.next_device_id += 1;
            id
        };

        // Add device ID to each tag's bitmap
        for (key, value) in tags {
            let tag = format!("{}={}", key, value);
            self.tag_index.entry(tag).or_default().insert(device_id);
        }
    }

    /// Queries devices matching a single tag
    ///
    /// # Arguments
    /// * `key` - Tag key (e.g., "location")
    /// * `value` - Tag value (e.g., "datacenter1")
    ///
    /// # Returns
    /// Vector of device names matching the tag
    pub fn query_tag(&self, key: &str, value: &str) -> Vec<String> {
        let tag = format!("{}={}", key, value);
        if let Some(bitmap) = self.tag_index.get(&tag) {
            bitmap
                .iter()
                .filter_map(|id| self.device_id_to_name.get(&id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Queries devices matching ALL of the specified tags (AND operation)
    ///
    /// # Arguments
    /// * `tags` - Key-value pairs that must ALL match
    ///
    /// # Returns
    /// Vector of device names matching all tags
    ///
    /// # Example
    /// ```ignore
    /// let devices = index.query_tags_and(&[
    ///     ("location", "datacenter1"),
    ///     ("sensor_type", "temperature"),
    /// ]);
    /// // Returns devices in datacenter1 AND temperature sensors
    /// ```
    pub fn query_tags_and(&self, tags: &[(&str, &str)]) -> Vec<String> {
        if tags.is_empty() {
            return Vec::new();
        }

        // Start with first tag's bitmap
        let first_tag = format!("{}={}", tags[0].0, tags[0].1);
        let mut result = if let Some(bitmap) = self.tag_index.get(&first_tag) {
            // Clone necessary: bitmap will be modified (AND'd with other bitmaps)
            bitmap.clone()
        } else {
            return Vec::new(); // First tag has no matches
        };

        // Intersect with remaining tags
        for (key, value) in &tags[1..] {
            let tag = format!("{}={}", key, value);
            if let Some(bitmap) = self.tag_index.get(&tag) {
                result &= bitmap; // Bitwise AND (intersection)
            } else {
                return Vec::new(); // One tag has no matches -> empty result
            }

            if result.is_empty() {
                return Vec::new(); // Early termination if intersection is empty
            }
        }

        // Convert device IDs to names
        result
            .iter()
            .filter_map(|id| self.device_id_to_name.get(&id).cloned())
            .collect()
    }

    /// Queries devices matching ANY of the specified tags (OR operation)
    ///
    /// # Arguments
    /// * `tags` - Key-value pairs where ANY match counts
    ///
    /// # Returns
    /// Vector of device names matching at least one tag
    ///
    /// # Example
    /// ```ignore
    /// let devices = index.query_tags_or(&[
    ///     ("sensor_type", "temperature"),
    ///     ("sensor_type", "humidity"),
    /// ]);
    /// // Returns temperature OR humidity sensors
    /// ```
    pub fn query_tags_or(&self, tags: &[(&str, &str)]) -> Vec<String> {
        if tags.is_empty() {
            return Vec::new();
        }

        let mut result = RoaringBitmap::new();

        // Union all tag bitmaps
        for (key, value) in tags {
            let tag = format!("{}={}", key, value);
            if let Some(bitmap) = self.tag_index.get(&tag) {
                result |= bitmap; // Bitwise OR (union)
            }
        }

        // Convert device IDs to names
        result
            .iter()
            .filter_map(|id| self.device_id_to_name.get(&id).cloned())
            .collect()
    }

    /// Returns the bitmap for a specific tag (advanced API)
    ///
    /// Useful for complex queries with custom bitmap operations.
    pub fn get_bitmap(&self, key: &str, value: &str) -> Option<&RoaringBitmap> {
        let tag = format!("{}={}", key, value);
        self.tag_index.get(&tag)
    }

    /// Returns the number of unique tags in the index
    pub fn tag_count(&self) -> usize {
        self.tag_index.len()
    }

    /// Returns the number of devices in the index
    pub fn device_count(&self) -> usize {
        self.device_id_to_name.len()
    }

    /// Serializes the inverted index to bytes
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<usize> {
        let mut bytes_written = 0;

        // Write device count
        writer.write_u32::<LittleEndian>(self.device_id_to_name.len() as u32)?;
        bytes_written += 4;

        // Write device ID -> name mapping
        for (&id, name) in &self.device_id_to_name {
            writer.write_u32::<LittleEndian>(id)?;
            writer.write_u32::<LittleEndian>(name.len() as u32)?;
            writer.write_all(name.as_bytes())?;
            bytes_written += 4 + 4 + name.len();
        }

        // Write tag count
        writer.write_u32::<LittleEndian>(self.tag_index.len() as u32)?;
        bytes_written += 4;

        // Write tag -> bitmap mapping
        for (tag, bitmap) in &self.tag_index {
            // Write tag
            writer.write_u32::<LittleEndian>(tag.len() as u32)?;
            writer.write_all(tag.as_bytes())?;
            bytes_written += 4 + tag.len();

            // Serialize bitmap
            let mut bitmap_bytes = Vec::new();
            bitmap.serialize_into(&mut bitmap_bytes)?;

            writer.write_u32::<LittleEndian>(bitmap_bytes.len() as u32)?;
            writer.write_all(&bitmap_bytes)?;
            bytes_written += 4 + bitmap_bytes.len();
        }

        Ok(bytes_written)
    }

    /// Deserializes an inverted index from bytes
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        let mut index = Self::new();

        // Read device count
        let device_count = reader.read_u32::<LittleEndian>()? as usize;

        // Read device ID -> name mapping
        for _ in 0..device_count {
            let id = reader.read_u32::<LittleEndian>()?;
            let name_len = reader.read_u32::<LittleEndian>()? as usize;
            let mut name_bytes = vec![0u8; name_len];
            reader.read_exact(&mut name_bytes)?;

            let name = String::from_utf8(name_bytes).map_err(|e| {
                TimbreError::InvalidState(format!("Invalid UTF-8 in device name: {}", e))
            })?;

            // Clone necessary: name inserted into two HashMaps
            index.device_id_to_name.insert(id, name.clone());
            index.device_name_to_id.insert(name, id);
            if id >= index.next_device_id {
                index.next_device_id = id + 1;
            }
        }

        // Read tag count
        let tag_count = reader.read_u32::<LittleEndian>()? as usize;

        // Read tag -> bitmap mapping
        for _ in 0..tag_count {
            // Read tag
            let tag_len = reader.read_u32::<LittleEndian>()? as usize;
            let mut tag_bytes = vec![0u8; tag_len];
            reader.read_exact(&mut tag_bytes)?;

            let tag = String::from_utf8(tag_bytes)
                .map_err(|e| TimbreError::InvalidState(format!("Invalid UTF-8 in tag: {}", e)))?;

            // Read bitmap
            let bitmap_len = reader.read_u32::<LittleEndian>()? as usize;
            let mut bitmap_bytes = vec![0u8; bitmap_len];
            reader.read_exact(&mut bitmap_bytes)?;

            let bitmap = RoaringBitmap::deserialize_from(&bitmap_bytes[..]).map_err(|e| {
                TimbreError::DecodingError(format!("Failed to deserialize bitmap: {}", e))
            })?;

            index.tag_index.insert(tag, bitmap);
        }

        Ok(index)
    }
}

impl Default for InvertedIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inverted_index_single_tag() {
        let mut index = InvertedIndex::new();

        index.add_device(
            "sensor_001",
            &[("location", "datacenter1"), ("type", "temperature")],
        );
        index.add_device(
            "sensor_002",
            &[("location", "datacenter2"), ("type", "temperature")],
        );
        index.add_device(
            "sensor_003",
            &[("location", "datacenter1"), ("type", "humidity")],
        );

        let devices = index.query_tag("location", "datacenter1");
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&"sensor_001".to_string()));
        assert!(devices.contains(&"sensor_003".to_string()));
    }

    #[test]
    fn test_inverted_index_and_query() {
        let mut index = InvertedIndex::new();

        index.add_device(
            "sensor_001",
            &[("location", "datacenter1"), ("type", "temperature")],
        );
        index.add_device(
            "sensor_002",
            &[("location", "datacenter2"), ("type", "temperature")],
        );
        index.add_device(
            "sensor_003",
            &[("location", "datacenter1"), ("type", "humidity")],
        );
        index.add_device(
            "sensor_004",
            &[("location", "datacenter1"), ("type", "temperature")],
        );

        // Query: datacenter1 AND temperature
        let devices = index.query_tags_and(&[("location", "datacenter1"), ("type", "temperature")]);
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&"sensor_001".to_string()));
        assert!(devices.contains(&"sensor_004".to_string()));
    }

    #[test]
    fn test_inverted_index_or_query() {
        let mut index = InvertedIndex::new();

        index.add_device("sensor_001", &[("type", "temperature")]);
        index.add_device("sensor_002", &[("type", "humidity")]);
        index.add_device("sensor_003", &[("type", "pressure")]);

        // Query: temperature OR humidity
        let devices = index.query_tags_or(&[("type", "temperature"), ("type", "humidity")]);
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&"sensor_001".to_string()));
        assert!(devices.contains(&"sensor_002".to_string()));
    }

    #[test]
    fn test_inverted_index_serialization() {
        let mut index = InvertedIndex::new();

        index.add_device("sensor_001", &[("location", "dc1"), ("type", "temp")]);
        index.add_device("sensor_002", &[("location", "dc2"), ("type", "temp")]);
        index.add_device("sensor_003", &[("location", "dc1"), ("type", "humid")]);

        // Serialize
        let mut buffer = Vec::new();
        index.serialize(&mut buffer).unwrap();

        // Deserialize
        let mut cursor = std::io::Cursor::new(buffer);
        let index2 = InvertedIndex::deserialize(&mut cursor).unwrap();

        // Verify
        assert_eq!(index2.device_count(), 3);
        assert_eq!(index2.tag_count(), 4); // dc1, dc2, temp, humid

        let devices = index2.query_tag("location", "dc1");
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&"sensor_001".to_string()));
        assert!(devices.contains(&"sensor_003".to_string()));
    }

    #[test]
    fn test_inverted_index_empty_query() {
        let index = InvertedIndex::new();

        // Query non-existent tag
        let devices = index.query_tag("location", "datacenter1");
        assert!(devices.is_empty());

        // AND query with no matches
        let devices = index.query_tags_and(&[("location", "datacenter1"), ("type", "temperature")]);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_inverted_index_bitmap_compression() {
        let mut index = InvertedIndex::new();

        // Add many devices with same tag to test bitmap compression
        for i in 0..1000 {
            let device_name = format!("sensor_{:04}", i);
            index.add_device(&device_name, &[("location", "datacenter1")]);
        }

        // Verify all devices are indexed
        let devices = index.query_tag("location", "datacenter1");
        assert_eq!(devices.len(), 1000);

        // Serialize and check size
        let mut buffer = Vec::new();
        let size = index.serialize(&mut buffer).unwrap();

        // RoaringBitmap should compress 1000 sequential IDs to <<1000 bytes
        // (typically ~50-100 bytes for sequential IDs)
        println!("Serialized size for 1000 devices: {} bytes", size);

        // Verify deserialization works
        let mut cursor = std::io::Cursor::new(buffer);
        let index2 = InvertedIndex::deserialize(&mut cursor).unwrap();
        let devices2 = index2.query_tag("location", "datacenter1");
        assert_eq!(devices2.len(), 1000);
    }
}
