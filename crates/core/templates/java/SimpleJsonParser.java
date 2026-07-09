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
        StringBuilder sb = new StringBuilder();
        while (true) {
            char c = s.charAt(pos[0]);
            if (c == '"') { pos[0]++; return sb.toString(); }
            if (c == '\\') {
                char n = s.charAt(pos[0] + 1);
                switch (n) {
                    case '"':  sb.append('"');  pos[0] += 2; break;
                    case '\\': sb.append('\\'); pos[0] += 2; break;
                    case '/':  sb.append('/');  pos[0] += 2; break;
                    case 'n':  sb.append('\n'); pos[0] += 2; break;
                    case 'r':  sb.append('\r'); pos[0] += 2; break;
                    case 't':  sb.append('\t'); pos[0] += 2; break;
                    case 'b':  sb.append('\b'); pos[0] += 2; break;
                    case 'f':  sb.append('\f'); pos[0] += 2; break;
                    case 'u':
                        sb.append((char) Integer.parseInt(s.substring(pos[0] + 2, pos[0] + 6), 16));
                        pos[0] += 6; break;
                    default:   sb.append(n); pos[0] += 2; break;
                }
            } else {
                sb.append(c);
                pos[0]++;
            }
        }
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
