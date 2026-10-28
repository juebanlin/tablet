package {{PACKAGE}};

import java.util.*;
import java.util.regex.Pattern;

@SuppressWarnings("unchecked")
public enum Paradigm {
    INT, LONG, FLOAT, DOUBLE, BOOL, STR,
    LIST, SET, MAP,
    TUPLE2, TUPLE3, TUPLE4,
    LIST_TUPLE2, LIST_TUPLE3, LIST_TUPLE4,
    MAP_TUPLE2, MAP_TUPLE3, MAP_TUPLE4, MAP_LIST,
    ;

    /**
     * 按类型的**结构**判定（根类型 + 泛型参数），不做文本模板通配匹配。
     *
     * 旧实现用 "List<?>" 这类模板逐条匹配，? 会吞掉任意字符串：
     * List<Tuple2<str,str>> 同时命中 LIST 和 LIST_TUPLE2，先声明的 LIST 胜出，
     * 解析结果退化成 List<String>，业务侧转 Tuple2 时 ClassCastException。
     * 结构化判定没有歧义，也不需要靠枚举顺序。
     */
    public static Paradigm of(String tblType) {
        try {
            return ofNode(TypeNode.parse(tblType));
        } catch (RuntimeException e) {
            return STR; // 畸形类型不在这里抛：导出侧已校验，运行时按字符串兜底
        }
    }

    private static Paradigm ofNode(TypeNode t) {
        switch (t.name) {
            case "int": return INT;
            case "long": return LONG;
            case "float": return FLOAT;
            case "double": return DOUBLE;
            case "bool": return BOOL;
            case "str": case "txt": return STR;
            case "Set": return SET;
            case "List":
                if (t.args.size() == 1) {
                    String inner = t.args.get(0).name;
                    if ("Tuple2".equals(inner)) return LIST_TUPLE2;
                    if ("Tuple3".equals(inner)) return LIST_TUPLE3;
                    if ("Tuple4".equals(inner)) return LIST_TUPLE4;
                }
                return LIST;
            case "Map":
                if (t.args.size() == 2) {
                    String val = t.args.get(1).name;
                    if ("Tuple2".equals(val)) return MAP_TUPLE2;
                    if ("Tuple3".equals(val)) return MAP_TUPLE3;
                    if ("Tuple4".equals(val)) return MAP_TUPLE4;
                    if ("List".equals(val)) return MAP_LIST;
                }
                return MAP;
            case "Tuple2": return TUPLE2;
            case "Tuple3": return TUPLE3;
            case "Tuple4": return TUPLE4;
            default: return STR;
        }
    }

    /** 类型字符串的结构化表示：根名 + 泛型参数（基础类型没有参数）。 */
    private static final class TypeNode {
        final String name;
        final List<TypeNode> args;

        TypeNode(String name, List<TypeNode> args) {
            this.name = name;
            this.args = args;
        }

        static TypeNode parse(String type) {
            String t = type == null ? "" : type.trim();
            int lt = t.indexOf('<');
            if (lt < 0) return new TypeNode(t, new ArrayList<TypeNode>());
            int rt = t.lastIndexOf('>');
            if (rt < lt) throw new IllegalArgumentException("非法类型: " + type);
            String name = t.substring(0, lt).trim();
            List<TypeNode> args = new ArrayList<>();
            for (String part : splitTopLevel(t.substring(lt + 1, rt), ',')) {
                String p = part.trim();
                if (!p.isEmpty()) args.add(parse(p));
            }
            return new TypeNode(name, args);
        }
    }

    /** 按顶层分隔符切分，忽略 <> 内部的分隔符。 */
    private static List<String> splitTopLevel(String s, char delimiter) {
        List<String> parts = new ArrayList<>();
        int depth = 0;
        int start = 0;
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '<') depth++;
            else if (c == '>') depth--;
            else if (c == delimiter && depth == 0) {
                parts.add(s.substring(start, i));
                start = i + 1;
            }
        }
        parts.add(s.substring(start));
        return parts;
    }

    public Object defaultValue() {
        switch (this) {
            case INT: return 0;
            case LONG: return 0L;
            case FLOAT: return 0f;
            case DOUBLE: return 0.0;
            case BOOL: return false;
            case STR: return "";
            case LIST: case LIST_TUPLE2: case LIST_TUPLE3: case LIST_TUPLE4:
                return new ArrayList<>();
            case SET: return new HashSet<>();
            case MAP: case MAP_TUPLE2: case MAP_TUPLE3: case MAP_TUPLE4: case MAP_LIST:
                return new HashMap<>();
            case TUPLE2: return new {{PACKAGE}}.types.Tuple2<>(0, 0);
            case TUPLE3: return new {{PACKAGE}}.types.Tuple3<>(0, 0, 0);
            case TUPLE4: return new {{PACKAGE}}.types.Tuple4<>(0, 0, 0, 0);
            default: return null;
        }
    }

    public Object parse(String raw, String tblType, SepConfig sep) {
        String[] innerTypes = extractInnerTypes(tblType);
        switch (this) {
            case INT: return Integer.parseInt(raw);
            case LONG: return Long.parseLong(raw);
            case FLOAT: return Float.parseFloat(raw);
            case DOUBLE: return Double.parseDouble(raw);
            case BOOL: return "true".equals(raw) || "1".equals(raw);
            case STR: return raw;
            case LIST: return splitPrimitives(raw, sep.list, innerTypes[0]);
            case SET: return new HashSet<>(splitPrimitives(raw, sep.set, innerTypes[0]));
            case MAP: return parseMap(raw, innerTypes, sep.mapKv, sep.mapEntry);
            case TUPLE2: return parseTuple(raw, innerTypes, sep.tuple2, 2);
            case TUPLE3: return parseTuple(raw, innerTypes, sep.tuple3, 3);
            case TUPLE4: return parseTuple(raw, innerTypes, sep.tuple4, 4);
            case LIST_TUPLE2: return parseListTuple(raw, innerTypes, sep.listTuple2Tuple, sep.listTuple2List, 2);
            case LIST_TUPLE3: return parseListTuple(raw, innerTypes, sep.listTuple3Tuple, sep.listTuple3List, 3);
            case LIST_TUPLE4: return parseListTuple(raw, innerTypes, sep.listTuple4Tuple, sep.listTuple4List, 4);
            case MAP_TUPLE2: return parseMapTuple(raw, innerTypes, sep.mapTuple2Kv, sep.mapTuple2Tuple, sep.mapTuple2Entry, 2);
            case MAP_TUPLE3: return parseMapTuple(raw, innerTypes, sep.mapTuple3Kv, sep.mapTuple3Tuple, sep.mapTuple3Entry, 3);
            case MAP_TUPLE4: return parseMapTuple(raw, innerTypes, sep.mapTuple4Kv, sep.mapTuple4Tuple, sep.mapTuple4Entry, 4);
            case MAP_LIST: return parseMapList(raw, innerTypes, sep.mapListKv, sep.mapListItem, sep.mapListEntry);
            default: return raw;
        }
    }

    /**
     * 取出类型里所有**叶子类型**（按出现顺序）。
     *
     * 和 {@link #of} 共用同一份结构解析：List<Tuple2<str,str>> 的叶子是
     * ["str","str"]，不是按逗号硬切出来的 ["Tuple2<str","str>"]。
     */
    private static String[] extractInnerTypes(String tblType) {
        List<String> out = new ArrayList<>();
        try {
            collectLeaves(TypeNode.parse(tblType), out);
        } catch (RuntimeException ignored) {
            // 畸形类型：交给 parse() 的默认分支按字符串处理
        }
        return out.toArray(new String[0]);
    }

    private static void collectLeaves(TypeNode t, List<String> out) {
        if (t.args.isEmpty()) {
            out.add(t.name);
            return;
        }
        for (TypeNode arg : t.args) collectLeaves(arg, out);
    }

    private static List<Object> splitPrimitives(String raw, String sep, String elemType) {
        List<Object> list = new ArrayList<>();
        for (String s : raw.split(Pattern.quote(sep))) {
            String t = s.trim();
            if (!t.isEmpty()) list.add(parsePrimitive(t, elemType));
        }
        return list;
    }

    private static Map<Object, Object> parseMap(String raw, String[] types, String kvSep, String entrySep) {
        Map<Object, Object> map = new LinkedHashMap<>();
        for (String entry : raw.split(Pattern.quote(entrySep))) {
            String e = entry.trim();
            if (e.isEmpty()) continue;
            int idx = e.indexOf(kvSep);
            if (idx > 0) {
                map.put(parsePrimitive(e.substring(0, idx).trim(), types[0]),
                        parsePrimitive(e.substring(idx + kvSep.length()).trim(), types[1]));
            }
        }
        return map;
    }

    private static Object parseTuple(String raw, String[] types, String sep, int size) {
        String[] parts = raw.split(Pattern.quote(sep), size);
        Object[] vals = new Object[size];
        for (int i = 0; i < size; i++) {
            vals[i] = i < parts.length ? parsePrimitive(parts[i].trim(), types[i]) : 0;
        }
        switch (size) {
            case 2: return new {{PACKAGE}}.types.Tuple2<>(vals[0], vals[1]);
            case 3: return new {{PACKAGE}}.types.Tuple3<>(vals[0], vals[1], vals[2]);
            default: return new {{PACKAGE}}.types.Tuple4<>(vals[0], vals[1], vals[2], vals[3]);
        }
    }

    private static List<Object> parseListTuple(String raw, String[] types, String tupleSep, String listSep, int tupleSize) {
        List<Object> list = new ArrayList<>();
        for (String item : raw.split(Pattern.quote(listSep))) {
            String t = item.trim();
            if (!t.isEmpty()) list.add(parseTuple(t, types, tupleSep, tupleSize));
        }
        return list;
    }

    private static Map<Object, Object> parseMapTuple(String raw, String[] types, String kvSep, String tupleSep, String entrySep, int tupleSize) {
        Map<Object, Object> map = new LinkedHashMap<>();
        String[] valTypes = Arrays.copyOfRange(types, 1, types.length);
        for (String entry : raw.split(Pattern.quote(entrySep))) {
            String e = entry.trim();
            if (e.isEmpty()) continue;
            int idx = e.indexOf(kvSep);
            if (idx > 0) {
                Object key = parsePrimitive(e.substring(0, idx).trim(), types[0]);
                Object val = parseTuple(e.substring(idx + kvSep.length()).trim(), valTypes, tupleSep, tupleSize);
                map.put(key, val);
            }
        }
        return map;
    }

    private static Map<Object, Object> parseMapList(String raw, String[] types, String kvSep, String itemSep, String entrySep) {
        Map<Object, Object> map = new LinkedHashMap<>();
        for (String entry : raw.split(Pattern.quote(entrySep))) {
            String e = entry.trim();
            if (e.isEmpty()) continue;
            int idx = e.indexOf(kvSep);
            if (idx > 0) {
                Object key = parsePrimitive(e.substring(0, idx).trim(), types[0]);
                List<Object> val = splitPrimitives(e.substring(idx + kvSep.length()).trim(), itemSep, types[1]);
                map.put(key, val);
            }
        }
        return map;
    }

    private static Object parsePrimitive(String raw, String type) {
        if (raw == null || raw.isEmpty()) return 0;
        String t = type.trim();
        // 引用类型 @Xxx：元素始终是 int id（@S24 嵌套 ref 退化为 int）
        if (t.startsWith("@")) return Integer.parseInt(raw);
        switch (t) {
            case "int": return Integer.parseInt(raw);
            case "long": return Long.parseLong(raw);
            case "float": return Float.parseFloat(raw);
            case "double": return Double.parseDouble(raw);
            case "bool": return "true".equals(raw) || "1".equals(raw);
            default: return raw;
        }
    }
}