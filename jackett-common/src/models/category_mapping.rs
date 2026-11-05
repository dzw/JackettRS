/// Mirror of Jackett.Common/Models/CategoryMapping.cs
#[derive(Debug, Clone)]
pub struct CategoryMapping {
    pub tracker_category: String,
    pub tracker_category_desc: Option<String>,
    pub newz_nab_category: i32,
}

impl CategoryMapping {
    pub fn new(tracker_cat: String, tracker_cat_desc: Option<String>, newz_cat: i32) -> Self {
        Self {
            tracker_category: tracker_cat,
            tracker_category_desc: tracker_cat_desc,
            newz_nab_category: newz_cat,
        }
    }
}
