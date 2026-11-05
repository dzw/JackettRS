use once_cell::sync::Lazy;

use super::torznab_category::TorznabCategory;

/// Mirror of Jackett.Common/Models/TorznabCatType.cs
pub struct TorznabCatType;

macro_rules! cat {
    ($id:expr, $name:expr) => {
        TorznabCategory::new($id, $name)
    };
}

/// Full category tree, built once (mirrors the C# static constructor).
pub struct Cats;

static CONSOLE: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(1000, "Console");
    c.sub_categories = vec![
        cat!(1010, "Console/NDS"),
        cat!(1020, "Console/PSP"),
        cat!(1030, "Console/Wii"),
        cat!(1040, "Console/XBox"),
        cat!(1050, "Console/XBox 360"),
        cat!(1060, "Console/Wiiware"),
        cat!(1070, "Console/XBox 360 DLC"),
        cat!(1080, "Console/PS3"),
        cat!(1090, "Console/Other"),
        cat!(1110, "Console/3DS"),
        cat!(1120, "Console/PS Vita"),
        cat!(1130, "Console/WiiU"),
        cat!(1140, "Console/XBox One"),
        cat!(1180, "Console/PS4"),
    ];
    c
});

static MOVIES: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(2000, "Movies");
    c.sub_categories = vec![
        cat!(2010, "Movies/Foreign"),
        cat!(2020, "Movies/Other"),
        cat!(2030, "Movies/SD"),
        cat!(2040, "Movies/HD"),
        cat!(2045, "Movies/UHD"),
        cat!(2050, "Movies/BluRay"),
        cat!(2060, "Movies/3D"),
        cat!(2070, "Movies/DVD"),
        cat!(2080, "Movies/WEB-DL"),
    ];
    c
});

static AUDIO: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(3000, "Audio");
    c.sub_categories = vec![
        cat!(3010, "Audio/MP3"),
        cat!(3020, "Audio/Video"),
        cat!(3030, "Audio/Audiobook"),
        cat!(3040, "Audio/Lossless"),
        cat!(3050, "Audio/Other"),
        cat!(3060, "Audio/Foreign"),
    ];
    c
});

static PC: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(4000, "PC");
    c.sub_categories = vec![
        cat!(4010, "PC/0day"),
        cat!(4020, "PC/ISO"),
        cat!(4030, "PC/Mac"),
        cat!(4040, "PC/Mobile-Other"),
        cat!(4050, "PC/Games"),
        cat!(4060, "PC/Mobile-iOS"),
        cat!(4070, "PC/Mobile-Android"),
    ];
    c
});

static TV: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(5000, "TV");
    c.sub_categories = vec![
        cat!(5010, "TV/WEB-DL"),
        cat!(5020, "TV/Foreign"),
        cat!(5030, "TV/SD"),
        cat!(5040, "TV/HD"),
        cat!(5045, "TV/UHD"),
        cat!(5050, "TV/Other"),
        cat!(5060, "TV/Sport"),
        cat!(5070, "TV/Anime"),
        cat!(5080, "TV/Documentary"),
    ];
    c
});

static XXX: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(6000, "XXX");
    c.sub_categories = vec![
        cat!(6010, "XXX/DVD"),
        cat!(6020, "XXX/WMV"),
        cat!(6030, "XXX/XviD"),
        cat!(6040, "XXX/x264"),
        cat!(6045, "XXX/UHD"),
        cat!(6050, "XXX/Pack"),
        cat!(6060, "XXX/ImageSet"),
        cat!(6070, "XXX/Other"),
        cat!(6080, "XXX/SD"),
        cat!(6090, "XXX/WEB-DL"),
    ];
    c
});

static BOOKS: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(7000, "Books");
    c.sub_categories = vec![
        cat!(7010, "Books/Mags"),
        cat!(7020, "Books/EBook"),
        cat!(7030, "Books/Comics"),
        cat!(7040, "Books/Technical"),
        cat!(7050, "Books/Other"),
        cat!(7060, "Books/Foreign"),
    ];
    c
});

static OTHER: Lazy<TorznabCategory> = Lazy::new(|| {
    let mut c = cat!(8000, "Other");
    c.sub_categories = vec![cat!(8010, "Other/Misc"), cat!(8020, "Other/Hashed")];
    c
});

impl TorznabCatType {
    pub fn console() -> &'static TorznabCategory {
        &CONSOLE
    }
    pub fn movies() -> &'static TorznabCategory {
        &MOVIES
    }
    pub fn audio() -> &'static TorznabCategory {
        &AUDIO
    }
    pub fn pc() -> &'static TorznabCategory {
        &PC
    }
    pub fn tv() -> &'static TorznabCategory {
        &TV
    }
    pub fn xxx() -> &'static TorznabCategory {
        &XXX
    }
    pub fn books() -> &'static TorznabCategory {
        &BOOKS
    }
    pub fn other() -> &'static TorznabCategory {
        &OTHER
    }

    pub fn parent_cats() -> Vec<&'static TorznabCategory> {
        vec![&CONSOLE, &MOVIES, &AUDIO, &PC, &TV, &XXX, &BOOKS, &OTHER]
    }

    pub fn all_cats() -> Vec<&'static TorznabCategory> {
        let mut all = Vec::new();
        all.push(&*CONSOLE);
        all.extend(CONSOLE.sub_categories.iter());
        all.push(&*MOVIES);
        all.extend(MOVIES.sub_categories.iter());
        all.push(&*AUDIO);
        all.extend(AUDIO.sub_categories.iter());
        all.push(&*PC);
        all.extend(PC.sub_categories.iter());
        all.push(&*TV);
        all.extend(TV.sub_categories.iter());
        all.push(&*XXX);
        all.extend(XXX.sub_categories.iter());
        all.push(&*BOOKS);
        all.extend(BOOKS.sub_categories.iter());
        all.push(&*OTHER);
        all.extend(OTHER.sub_categories.iter());
        all
    }

    pub fn get_cat_desc(torznab_cat_id: i32) -> String {
        Self::all_cats()
            .into_iter()
            .find(|c| c.id == torznab_cat_id)
            .and_then(|c| c.name.clone())
            .unwrap_or_default()
    }

    pub fn get_cat_by_name(name: &str) -> Option<&'static TorznabCategory> {
        Self::all_cats().into_iter().find(|c| c.name.as_deref() == Some(name))
    }
}
