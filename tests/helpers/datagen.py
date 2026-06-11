#!/usr/bin/env python3
"""
tests/helpers/datagen.py — 随机 .tbl 数据生成器

被 shell 脚本调用，按 schema 结构生成指定行数的 .tbl 数据文件。

用法:
    python3 datagen.py --schema <path> --output <dir> --rows <N> [--seed <S>]
"""

import argparse
import os
import random
import re
import sys


def main():
    parser = argparse.ArgumentParser(description="生成随机 .tbl 测试数据")
    parser.add_argument("--schema", required=True, help=".tblschema 文件路径")
    parser.add_argument("--output", required=True, help="输出 config 目录")
    parser.add_argument("--rows", type=int, default=1000, help="每表数据行数")
    parser.add_argument("--seed", type=int, default=42, help="随机种子")
    args = parser.parse_args()

    random.seed(args.seed)
    sections = parse_schema(args.schema)

    for sec in sections:
        if sec["mode"] == "table":
            content = generate_table_tbl(sec, args.rows)
        elif sec["mode"] == "constant":
            content = generate_constant_tbl(sec)
        elif sec["mode"] == "enum":
            content = generate_enum_tbl(sec)
        else:
            continue

        group_dir = os.path.join(args.output, sec["group"])
        os.makedirs(group_dir, exist_ok=True)
        filepath = os.path.join(group_dir, f"{sec['name']}.tbl")
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(content)

    print(f"已生成 {len(sections)} 个 .tbl 文件（{args.rows} 行/表）到 {args.output}")


def parse_schema(path):
    """简单解析 .tblschema，提取 sections 结构"""
    sections = []
    current = None

    with open(path, "r", encoding="utf-8") as f:
        in_preset = False
        for line in f:
            line = line.rstrip("\n")

            # section header: [group/Name] table|constant|enum
            m = re.match(r"^\[(\w+)/(\w+)\]\s+(table|constant|enum)", line)
            if m:
                if current:
                    sections.append(current)
                current = {
                    "group": m.group(1),
                    "name": m.group(2),
                    "mode": m.group(3),
                    "fields": [],
                    "preset": [],
                }
                in_preset = False
                continue

            if current is None:
                continue

            if line.strip() == "# @preset":
                in_preset = True
                continue

            if in_preset:
                if line.startswith("[") or line.startswith("#!"):
                    in_preset = False
                else:
                    current["preset"].append(line.split("|"))
                    continue

            # field line for table: name | type | export | desc
            if current["mode"] == "table" and not line.startswith("#") and "|" in line:
                parts = [p.strip() for p in line.split("|")]
                if len(parts) >= 3:
                    current["fields"].append({
                        "name": parts[0],
                        "type": parts[1],
                        "export": parts[2],
                    })

        if current:
            sections.append(current)

    return sections


def generate_table_tbl(sec, rows):
    """生成 table 类型的 .tbl 内容"""
    fields = sec["fields"]
    lines = []
    lines.append("#!tbl v2")
    lines.append("#mode table")
    lines.append("#desc " + "|".join(f.get("name", "") for f in fields))
    lines.append("#export " + "|".join(f.get("export", "cs") for f in fields))
    lines.append("#type " + "|".join(f["type"] for f in fields))
    lines.append("#field " + "|".join(f["name"] for f in fields))
    lines.append("---")

    for i in range(rows):
        row = []
        for f in fields:
            row.append(random_value(f["type"], f["name"], i))
        lines.append("|".join(row))

    return "\n".join(lines) + "\n"


def generate_constant_tbl(sec):
    """生成 constant 类型的 .tbl"""
    lines = ["#!tbl v2", "#mode constant", "---"]
    for row in sec["preset"]:
        lines.append("|".join(row))
    return "\n".join(lines) + "\n"


def generate_enum_tbl(sec):
    """生成 enum 类型的 .tbl"""
    lines = ["#!tbl v2", "#mode enum", "---"]
    for row in sec["preset"]:
        lines.append("|".join(row))
    return "\n".join(lines) + "\n"


def random_value(tbl_type, field_name, row_idx):
    """按类型生成随机值"""
    if field_name == "id":
        return str(1001 + row_idx)
    if field_name == "name":
        names = ["战士", "法师", "弓手", "刺客", "牧师", "骑士", "猎人", "术士", "武僧", "德鲁伊",
                 "圣骑", "死灵", "元素", "召唤", "游侠", "机关", "剑圣", "忍者", "海盗", "学者"]
        return names[row_idx % len(names)] + str(row_idx // len(names))

    if tbl_type == "int":
        return str(random.randint(1, 9999))
    if tbl_type == "long":
        return str(random.randint(10000, 999999))
    if tbl_type in ("float", "double"):
        return f"{random.uniform(0.1, 100.0):.2f}"
    if tbl_type == "bool":
        return random.choice(["true", "false"])
    if tbl_type == "str":
        return f"text_{random.randint(1, 9999)}"

    # List<int>
    if tbl_type == "List<int>":
        n = random.randint(1, 5)
        return ";".join(str(random.randint(1, 100)) for _ in range(n))
    # List<str>
    if tbl_type == "List<str>":
        n = random.randint(1, 3)
        return ";".join(f"s{random.randint(1,99)}" for _ in range(n))
    # Set<int>
    if tbl_type == "Set<int>":
        n = random.randint(1, 4)
        vals = random.sample(range(1, 100), min(n, 99))
        return ";".join(str(v) for v in vals)
    # Map<str,int>
    if tbl_type == "Map<str,int>":
        n = random.randint(1, 3)
        return ";".join(f"k{i}:{random.randint(1,99)}" for i in range(n))
    # Map<int,int>
    if tbl_type == "Map<int,int>":
        n = random.randint(1, 3)
        return ";".join(f"{i+1}:{random.randint(1,99)}" for i in range(n))
    # Tuple2<int,int>
    if tbl_type.startswith("Tuple2"):
        return f"{random.randint(1,100)},{random.randint(1,100)}"
    # Tuple3
    if tbl_type.startswith("Tuple3"):
        return f"{random.randint(1,100)},{random.randint(1,100)},{random.randint(1,100)}"
    # Ref (@TableName)
    if tbl_type.startswith("@"):
        return str(random.randint(1001, 1010))

    # fallback
    return str(random.randint(1, 100))


if __name__ == "__main__":
    main()
