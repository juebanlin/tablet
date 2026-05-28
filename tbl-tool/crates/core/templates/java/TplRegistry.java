package {{PACKAGE}};

import java.util.*;

public class TplRegistry {
    private static final Map<String, Class<?>> map = new LinkedHashMap<>();

    static {
{{REGISTER_LIST}}
    }

    public static Map<String, Class<?>> getAll() {
        return Collections.unmodifiableMap(map);
    }

    public static Class<?> get(String key) {
        return map.get(key);
    }
}
