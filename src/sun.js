// Sunrise and sunset in local clock hours for the viewer's date. The place is guessed from the
// time zone (an IANA zone is named after a city), so nothing is asked for and no network is used.
const ZONES = {
  'Asia/Ho_Chi_Minh': [10.8, 106.7],
  'Asia/Saigon': [10.8, 106.7],
  'Asia/Bangkok': [13.75, 100.5],
  'Asia/Phnom_Penh': [11.55, 104.9],
  'Asia/Vientiane': [17.97, 102.6],
  'Asia/Jakarta': [-6.2, 106.8],
  'Asia/Singapore': [1.35, 103.8],
  'Asia/Kuala_Lumpur': [3.14, 101.7],
  'Asia/Manila': [14.6, 121],
  'Asia/Hong_Kong': [22.3, 114.2],
  'Asia/Shanghai': [31.2, 121.5],
  'Asia/Taipei': [25.03, 121.6],
  'Asia/Seoul': [37.57, 127],
  'Asia/Tokyo': [35.68, 139.7],
  'Asia/Kolkata': [22.57, 88.4],
  'Asia/Calcutta': [22.57, 88.4],
  'Asia/Dubai': [25.2, 55.3],
  'Europe/London': [51.5, -0.13],
  'Europe/Paris': [48.86, 2.35],
  'Europe/Berlin': [52.52, 13.4],
  'Europe/Madrid': [40.42, -3.7],
  'Europe/Rome': [41.9, 12.5],
  'Europe/Amsterdam': [52.37, 4.9],
  'Europe/Warsaw': [52.23, 21],
  'Europe/Prague': [50.08, 14.4],
  'Europe/Stockholm': [59.33, 18.07],
  'Europe/Helsinki': [60.17, 24.94],
  'Europe/Moscow': [55.76, 37.6],
  'America/New_York': [40.71, -74],
  'America/Chicago': [41.88, -87.6],
  'America/Denver': [39.74, -105],
  'America/Los_Angeles': [34.05, -118.2],
  'America/Toronto': [43.65, -79.4],
  'America/Vancouver': [49.28, -123.1],
  'America/Mexico_City': [19.43, -99.1],
  'America/Sao_Paulo': [-23.55, -46.6],
  'America/Buenos_Aires': [-34.6, -58.4],
  'Australia/Sydney': [-33.87, 151.2],
  'Australia/Melbourne': [-37.81, 145],
  'Pacific/Auckland': [-36.85, 174.8],
  'Africa/Cairo': [30.04, 31.2],
  'Africa/Johannesburg': [-26.2, 28],
};

const RAD = Math.PI / 180;

// [latitude, longitude] for a time zone. Unknown zones get the equator at the zone's own
// meridian: sunrise near 06:00 and sunset near 18:00 all year, as before.
export function placeFor(zone, date = new Date()) {
  return ZONES[zone] ?? [0, (-date.getTimezoneOffset() / 60) * 15];
}

export function viewerPlace(date = new Date()) {
  let zone;
  try {
    zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {}
  return placeFor(zone, date);
}

function dayOfYear(date) {
  return Math.round((new Date(date.getFullYear(), date.getMonth(), date.getDate()) - new Date(date.getFullYear(), 0, 0)) / 864e5);
}

// { rise, set } in local clock hours (NOAA's approximation, a few minutes off at most).
// Polar day and night are clamped to 1-23 h of daylight so the sky keeps a sunrise and a sunset.
export function sunTimes(date, [lat, lon]) {
  const n = dayOfYear(date);
  const g = ((2 * Math.PI) / 365) * (n - 1);
  const decl = 0.006918 - 0.399912 * Math.cos(g) + 0.070257 * Math.sin(g) - 0.006758 * Math.cos(2 * g) + 0.000907 * Math.sin(2 * g);
  const eqTime = 229.18 * (0.000075 + 0.001868 * Math.cos(g) - 0.032077 * Math.sin(g) - 0.014615 * Math.cos(2 * g) - 0.040849 * Math.sin(2 * g));
  const cosH = (Math.cos(90.833 * RAD) - Math.sin(lat * RAD) * Math.sin(decl)) / (Math.cos(lat * RAD) * Math.cos(decl));
  const half = Math.min(11.5, Math.max(0.5, Math.acos(Math.min(1, Math.max(-1, cosH))) / RAD / 15)); // hours
  const noon = 12 - eqTime / 60 - lon / 15 - date.getTimezoneOffset() / 60;
  return { rise: (noon - half + 24) % 24, set: (noon + half) % 24 };
}

// 0 in midsummer, 1 in midwinter (the fog season), flipped south of the equator.
export function winterness(date, lat) {
  const n = dayOfYear(date) + (lat < 0 ? 182 : 0);
  return 0.5 + 0.5 * Math.cos(((n - 15) / 365) * 2 * Math.PI);
}
