package {{PACKAGE}};

import java.util.*;

@SuppressWarnings("unchecked")
public class SimpleJsonParser implements IJsonParser {
    @Override
    public Map<String, Object> parseObject(String json) {
        return (Map<String, Object>) parse(json.trim(), new int[]{0});
    }

    private Object parse(String s, int[] pos) {
        skipWhitespace(s, pos);
        char c = s.charAt(pos[0]);
        if (c == '{') return parseObj(s, pos);
        if (c == '[') return parseArr(s, pos);
        if (c == '"') return parseStr(s, pos);
        if (c == 't' || c == 'f') return parseBool(s, pos);
        if (c == 'n') { pos[0] += 4; return null; }
        return parseNum(s, pos);
    }

    private Map<String, Object> parseObj(String s, int[] pos) {
        Map<String, Object> map = new LinkedHashMap<>();
        pos[0]++;
        skipWhitespace(s, pos);
        if (s.charAt(pos[0]) == '}') { pos[0]++; return map; }
        while (true) {
            skipWhitespace(s, pos);
            String key = parseStr(s, pos);
            skipWhitespace(s, pos);
            pos[0]++;
            Object val = parse(s, pos);
            map.put(key, val);
            skipWhitespace(s, pos);
            if (s.charAt(pos[0]) == '}') { pos[0]++; return map; }
            pos[0]++;
        }
    }

    private List<Object> parseArr(String s, int[] pos) {
        List<Object> list = new ArrayList<>();
        pos[0]++;
        skipWhitespace(s, pos);
        if (s.charAt(pos[0]) == ']') { pos[0]++; return list; }
        while (true) {
            list.add(parse(s, pos));
            skipWhitespace(s, pos);
            if (s.charAt(pos[0]) == ']') { pos[0]++; return list; }
            pos[0]++;
        }
    }

    private String parseStr(String s, int[] pos) {
        pos[0]++;
        int start = pos[0];
        while (s.charAt(pos[0]) != '"') pos[0]++;
        String r = s.substring(start, pos[0]);
        pos[0]++;
        return r;
    }

    private Number parseNum(String s, int[] pos) {
        int start = pos[0];
        boolean isFloat = false;
        while (pos[0] < s.length()) {
            char c = s.charAt(pos[0]);
            if (c == '.' || c == 'e' || c == 'E') isFloat = true;
            if (c == ',' || c == '}' || c == ']' || Character.isWhitespace(c)) break;
            pos[0]++;
        }
        String num = s.substring(start, pos[0]);
        if (isFloat) return Double.parseDouble(num);
        long v = Long.parseLong(num);
        if (v >= Integer.MIN_VALUE && v <= Integer.MAX_VALUE) return (int) v;
        return v;
    }

    private Boolean parseBool(String s, int[] pos) {
        if (s.charAt(pos[0]) == 't') { pos[0] += 4; return true; }
        pos[0] += 5; return false;
    }

    private void skipWhitespace(String s, int[] pos) {
        while (pos[0] < s.length() && Character.isWhitespace(s.charAt(pos[0]))) pos[0]++;
    }
}
