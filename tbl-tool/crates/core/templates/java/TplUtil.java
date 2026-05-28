package {{PACKAGE}};

import java.util.*;

public class TplUtil {

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

    public static Object parseField(String raw, String tblType, SepConfig sep) {
        Paradigm p = Paradigm.of(tblType);
        if (raw == null || raw.isEmpty()) return p.defaultValue();
        return p.parse(raw, tblType, sep);
    }
}
