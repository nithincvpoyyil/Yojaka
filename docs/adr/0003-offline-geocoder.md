# ADR-0003: Offline reverse geocoder

Status: accepted (Iteration 2)

## Context
FR-44 needs country, state, district and nearest town from GPS, fully offline (coordinates never leave the machine).

## Decision
- Data: GeoNames `cities1000` (populated places with population ≥ 1,000: 171,800 places), `admin1CodesASCII`, `admin2Codes`, `countryInfo`. License CC-BY 4.0: attribution is required in `--about` and the README (done: `yojaka --about`, README).
- `tools/geodata` converts the dumps into one zstd-compressed little-endian blob (layout documented in its source), written to `crates/yojaka-core/data/geo.bin.zst` (2.9 MB, committed). Only admin names that some place references are kept.
- The core embeds it with `include_bytes!`, decompresses it on first use (`OnceLock`) and builds a static 3-D k-d tree over unit-sphere coordinates. Using 3-D vectors makes the poles and the antimeridian ordinary cases. The nearest place is reported with its great-circle distance.
- Own k-d tree (about 60 lines, checked against brute force in a test) instead of `kiddo`, to avoid an API dependency for a static nearest-neighbour query.
- Results are cached in `meta.place_*` and recomputed when `EmbeddedGeocoder::DATA_VERSION` changes (stored in `meta_kv.geodata_version`).
- Precision guard (FR-45): `Place::guarded(near_km, far_km)` gives `City`, `Near City`, or nothing; the country is only used within 300 km.

## Measured
- 100,000 random lookups: 0.39 s in a release build including the one-time load (limit 1 s).
- Binary: 3.78 MB → 7.26 MB for all of Iteration 2 (limit +6 MB).

## Known limits
- Nearest-place lookup is not a border test. Within a few km of a border it can pick the town on the wrong side (Tijuana vs San Ysidro). Accepted for v1 and documented in the geo tests.
- GeoNames names neighbourhoods and districts too ("Paris 04 Hôtel-de-Ville", "Times Square", "Eifuku" for central Tokyo), so city names are only as good as the nearest GeoNames entry. A population filter or a city-level preference could improve this later.
