package {{PACKAGE}};

import java.util.*;
import java.util.regex.Pattern;

@SuppressWarnings("unchecked")
public enum Paradigm {
    INT("int"),
    LONG("long"),
    FLOAT("float"),
    DOUBLE("double"),
    BOOL("bool"),
    STR("str"),
    LIST("List<?>"),
    SET("Set<?>"),
    MAP("Map<?,?>"),
    TUPLE2("Tuple2<?,?>"),
    TUPLE3("Tuple3<?,?,?>"),
    TUPLE4("Tuple4<?,?,?,?>"),
    LIST_TUPLE2("List<Tuple2<?,?>>"),
    LIST_TUPLE3("List<Tuple3<?,?,?>>"),
    LIST_TUPLE4("List<Tuple4<?,?,?,?>>"),
    MAP_TUPLE2("Map<?,Tuple2<?,?>>"),
    MAP_TUPLE3("Map<?,Tuple3<?,?,?>>"),
    MAP_TUPLE4("Map<?,Tuple4<?,?,?,?>>"),
    MAP_LIST("Map<?,List<?>>"),
    ;

    private final String template;

    Paradigm(String template) {
        this.template = template;
    }

    public String template() { return template; }

    public static Paradigm of(String tblType) {
        // 从复杂到简单匹配，避免短模板误匹配
        for (Paradigm p : values()) {
            if (p.matches(tblType)) return p;
        }
        return STR;
    }

    private boolean matches(String tblType) {
        return matchTemplate(template, tblType, 0, 0);
    }

    private static boolean matchTemplate(String pattern, String input, int pi, int ii) {
        while (pi < pattern.length() && ii < input.length()) {
            char pc = pattern.charAt(pi);
            if (pc == '?') {
                // ? 匹配到下一个模板字符或结尾
                pi++;
                if (pi >= pattern.length()) return ii <= input.length();
                char next = pattern.charAt(pi);
                // 找 input 中下一个 next 字符的位置
                for (int k = ii; k < input.length(); k++) {
                    if (input.charAt(k) == next && matchTemplate(pattern, input, pi, k)) return true;
                }
                return false;
            } else {
                if (pc != input.charAt(ii)) return false;
                pi++;
                ii++;
            }
        }
        return pi == pattern.length() && ii == input.length();
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

    private String[] extractInnerTypes(String tblType) {
        int start = tblType.indexOf('<');
        if (start < 0) return new String[0];
        int end = tblType.lastIndexOf('>');
        String inner = tblType.substring(start + 1, end);
        return inner.split(",", -1);
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
        switch (type.trim()) {
            case "int": return Integer.parseInt(raw);
            case "long": return Long.parseLong(raw);
            case "float": return Float.parseFloat(raw);
            case "double": return Double.parseDouble(raw);
            case "bool": return "true".equals(raw) || "1".equals(raw);
            default: return raw;
        }
    }
}