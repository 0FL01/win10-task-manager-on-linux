#!/usr/bin/env python3

import argparse
import os
import sqlite3

parser = argparse.ArgumentParser(
    description="Create a hardware database file for network card models and manufacturers")
parser.add_argument("-o", "--output", type=str, help="output path")
parser.add_argument("files", nargs="+", type=str, help="input files .hwdb files")
args = parser.parse_args()

os.makedirs(args.output, exist_ok=True)
db_filename = args.output + '/hw.db'
os.remove(db_filename) if os.path.exists(db_filename) else None
conn = sqlite3.connect(db_filename)
cursor = conn.cursor()

cursor.execute("CREATE TABLE products (key TEXT PRIMARY KEY, value TEXT)")
cursor.execute("CREATE TABLE models (key TEXT PRIMARY KEY, value TEXT)")
cursor.execute("CREATE TABLE key_len (key TEXT PRIMARY KEY, value INTEGER)")

key_len_min = pow(2, 32)
key_len_max = 0

for filename in args.files:
    with open(filename, "r") as f:
        contents = f.read()
        f.close()

    lines = [line.strip() for line in contents.split("\n") if line.strip() and not line.startswith("#")]

    key = ''
    for line in lines:
        if line:
            if line.endswith('*'):
                key = line.split('*')[0]
                if len(key) < key_len_min:
                    key_len_min = len(key)
                if len(key) > key_len_max:
                    key_len_max = len(key)
                continue
            else:
                if not line.startswith("ID_"):
                    continue
                ty, value = line.split("=", 1)
                if ty == "ID_PRODUCT_FROM_DATABASE":
                    cursor.execute("INSERT INTO products (key, value) VALUES (?, ?)", (key.strip(), value.strip()))
                if ty == "ID_MODEL_FROM_DATABASE":
                    cursor.execute("INSERT INTO models (key, value) VALUES (?, ?)", (key.strip(), value.strip()))

cursor.execute("INSERT INTO key_len (key, value) VALUES (?, ?)", ('min', key_len_min))
cursor.execute("INSERT INTO key_len (key, value) VALUES (?, ?)", ('max', key_len_max))

conn.commit()
conn.close()
