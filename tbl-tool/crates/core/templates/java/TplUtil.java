package {{PACKAGE}};

import java.util.*;

@SuppressWarnings("unchecked")
public class TplUtil {
    public static int toInt(Object v) { return v == null ? 0 : ((Number) v).intValue(); }
    public static long toLong(Object v) { return v == null ? 0L : ((Number) v).longValue(); }
    public static float toFloat(Object v) { return v == null ? 0f : ((Number) v).floatValue(); }
    public static double toDouble(Object v) { return v == null ? 0.0 : ((Number) v).doubleValue(); }
    public static boolean toBool(Object v) { return v != null && (Boolean) v; }
    public static String toStr(Object v) { return v == null ? "" : v.toString(); }

    public static Object defaultValue(Class<?> type) {
        if (type == int.class) return 0;
        if (type == long.class) return 0L;
        if (type == float.class) return 0f;
        if (type == double.class) return 0.0;
        if (type == boolean.class) return false;
        if (type == String.class) return "";
        if (type == List.class) return new ArrayList<>();
        if (type == Set.class) return new HashSet<>();
        if (type == Map.class) return new HashMap<>();
        return null;
    }

    public static Object convert(Object val, Class<?> type) {
        if (val == null) return null;
        if (type == int.class || type == Integer.class) return ((Number) val).intValue();
        if (type == long.class || type == Long.class) return ((Number) val).longValue();
        if (type == float.class || type == Float.class) return ((Number) val).floatValue();
        if (type == double.class || type == Double.class) return ((Number) val).doubleValue();
        if (type == boolean.class || type == Boolean.class) return val;
        if (type == String.class) return val.toString();
        if (type == List.class) return val;
        if (type == Set.class) return new HashSet<>((List<?>) val);
        if (type == Map.class) return val;
        if ({{PACKAGE}}.types.Tuple2.class.isAssignableFrom(type)) {
            List<?> arr = (List<?>) val;
            return new {{PACKAGE}}.types.Tuple2<>(arr.get(0), arr.get(1));
        }
        if ({{PACKAGE}}.types.Tuple3.class.isAssignableFrom(type)) {
            List<?> arr = (List<?>) val;
            return new {{PACKAGE}}.types.Tuple3<>(arr.get(0), arr.get(1), arr.get(2));
        }
        if ({{PACKAGE}}.types.Tuple4.class.isAssignableFrom(type)) {
            List<?> arr = (List<?>) val;
            return new {{PACKAGE}}.types.Tuple4<>(arr.get(0), arr.get(1), arr.get(2), arr.get(3));
        }
        return val;
    }
}
