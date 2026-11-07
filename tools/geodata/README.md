# geodata

Builds the embedded GeoNames blob used for offline reverse geocoding (ADR-0003).

```sh
mkdir dumps && cd dumps
for f in cities1000.zip admin1CodesASCII.txt admin2Codes.txt countryInfo.txt; do
  curl -sSfO https://download.geonames.org/export/dump/$f
done
unzip cities1000.zip && cd ..
cargo run -p geodata -- dumps crates/yojaka-core/data/geo.bin.zst
```

If the blob layout or the data changes, bump `EmbeddedGeocoder::DATA_VERSION` so cached places are recomputed.

License of the data: CC-BY 4.0 (GeoNames, <https://www.geonames.org/>). Attribution is required.
