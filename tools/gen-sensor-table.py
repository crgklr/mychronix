#!/usr/bin/env python3
"""Regenerates crates/aim-xrk/src/sensor_table.rs from RaceStudio 2's sensor
table (FW_SENSORI in DATABASE/Configs_20800AIM.mdb of an RS2 install).

    pip install access-parser
    tools/gen-sensor-table.py /path/to/Configs_20800AIM.mdb > crates/aim-xrk/src/sensor_table.rs
"""
import sys

from access_parser import AccessParser

db = AccessParser(sys.argv[1])


def rows(table):
    t = db.parse_table(table)
    cols = list(t)
    return [{c: t[c][i] for c in cols} for i in range(len(t[cols[0]]))]


units = {r["CODICE_UNITA_BASE"]: (r["STRINGA_UNITAMISURA"] or "").strip() for r in rows("FW_UNITABASE")}
rename = {"°C": "C", "°F": "F", "#": ""}

print("""//! AiM sensor scaling, by sensor code: generated from RaceStudio 2's
//! `FW_SENSORI` table (`DATABASE/Configs_20800AIM.mdb`) by
//! `tools/gen-sensor-table.py`. Kind 1: `a1 * raw + a0`; kind 2 (pulse
//! inputs): `a1 * raw`; kind 0: calibrated per channel (accelerometers,
//! potentiometers, custom sensors) or fixed scale (battery); 101-104: lambda curves.

/// (code, kind, a1, a0, units)
pub(crate) const SENSORS: &[(u16, u8, f64, f64, &str)] = &[""")
for r in sorted(rows("FW_SENSORI"), key=lambda r: r["CODICE_SENSORE"]):
    u = units.get(r["CODICE_UNITMIS_BASE"], "") or ""
    u = rename.get(u, u).replace('"', "")
    a1, a0 = float(r["COEFF_A1"] or 0.0), float(r["COEFF_A0"] or 0.0)
    print(f'    ({r["CODICE_SENSORE"]}, {r["FUNZIONE_O_SPEZZATA"]}, {a1!r}, {a0!r}, "{u}"),')
print("];")
