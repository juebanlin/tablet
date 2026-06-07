package {{PACKAGE}};

import java.util.*;

public class TblMeta {
    public final String key;
    public final Class<?> clazz;
    public final boolean isTable;
    public final SepConfig sep;
    public final List<Map<String, String>> rows;
    public final Map<String, String> constData;

    private TblMeta(String key, Class<?> clazz, boolean isTable, SepConfig sep,
                    List<Map<String, String>> rows, Map<String, String> constData) {
        this.key = key;
        this.clazz = clazz;
        this.isTable = isTable;
        this.sep = sep;
        this.rows = rows;
        this.constData = constData;
    }

    public static TblMeta table(String key, Class<?> clazz, SepConfig sep, List<Map<String, String>> rows) {
        return new TblMeta(key, clazz, true, sep, rows, null);
    }

    public static TblMeta constant(String key, Class<?> clazz, SepConfig sep, Map<String, String> data) {
        return new TblMeta(key, clazz, false, sep, null, data);
    }
}
