use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct ImageRecord {
    pub path: PathBuf,
    pub filename: String,
    pub subfolder: String,
    pub file_size: u64,
    pub mtime: f64,
    pub ctime: f64,
    pub width: u32,
    pub height: u32,
    pub thumb_path: PathBuf,
    pub is_favorite: bool,
    pub aspect: AspectFilter,
    pub color_name: String,
    pub dhash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AspectFilter {
    #[default]
    All,
    Portrait,
    Landscape,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
    NameAsc,
    NameDesc,
    SizeLargest,
    SizeSmallest,
    Random,
}
