// Standard Torznab category id -> name mapping (subset of TorznabCatType).
// Manual search results carry numeric category ids; the UI shows names like
// the C# dashboard does.
const MAP = {
  1000: "Console",
  1010: "Console/NDS",
  1020: "Console/PSP",
  1030: "Console/Wii",
  1040: "Console/XBox",
  1050: "Console/XBox 360",
  1060: "Console/Wiiware",
  1070: "Console/XBox 360 DLC",
  1080: "Console/PS3",
  1090: "Console/Other",
  1110: "Console/3DS",
  1120: "Console/PS Vita",
  1130: "Console/Wii U",
  1140: "Console/Xbox One",
  1150: "Console/PS4",
  1160: "Console/Other OS",
  2000: "Movies",
  2010: "Movies/Foreign",
  2020: "Movies/SD",
  2030: "Movies/HD",
  2040: "Movies/UMD",
  2050: "Movies/BluRay",
  2060: "Movies/DVD",
  2070: "Movies/WEB-DL",
  2080: "Movies/3D",
  2090: "Movies/BluRay UHD",
  4000: "PC",
  4010: "PC/0day",
  4020: "PC/ISO",
  4030: "PC/Mac",
  4040: "PC/Mobile-Other",
  4050: "PC/Games",
  4060: "PC/Mobile-iOS",
  4070: "PC/Mobile-Android",
  5000: "TV",
  5010: "TV/WEB-DL",
  5020: "TV/Foreign",
  5030: "TV/SD",
  5040: "TV/HD",
  5050: "TV/Other",
  5060: "TV/Sport",
  5070: "TV/Anime",
  5080: "TV/Documentary",
  6000: "XXX",
  6010: "XXX/DVD",
  6020: "XXX/WEB-DL",
  6030: "XXX/SD",
  6040: "XXX/HD",
  6050: "XXX/UHD",
  6060: "XXX/Other",
  6070: "XXX/Pack",
  6080: "XXX/ImageSet",
  7000: "Books",
  7010: "Books/Magazines",
  7020: "Books/EBook",
  7030: "Books/Comics",
  8000: "Other",
  8010: "Other/Misc",
  8020: "Other/Hashed",
};

export function categoryName(id) {
  return MAP[id] || String(id);
}

// Renders a release's category id list as display names.
export function categoryNames(release) {
  const ids = release.category || [];
  return ids.map(categoryName).join(", ");
}
