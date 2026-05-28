package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

@SuppressWarnings("unchecked")
public class TplHolder {
    private static final Map<Class<?>, Map<Integer, ? extends ITpl>> tables = new HashMap<>();
    private static final Map<Class<?>, IConstTpl> constants = new HashMap<>();
    private static final Map<String, Class<?>> registry = new HashMap<>();
    private static IJsonParser parser = new SimpleJsonParser();
    private static String basePackage = "{{PACKAGE}}";
    private static String dataDir;

    static {
{{REGISTER_LIST}}
    }

    public static void init(String dir) {
        init(dir, new SimpleJsonParser());
    }

    public static void init(String dir, IJsonParser jsonParser) {
        dataDir = dir;
        parser = jsonParser;
        loadAll();
    }

    private static void loadAll() {
        try {
            Files.walk(Paths.get(dataDir))
                .filter(p -> p.toString().endsWith(".json"))
                .forEach(p -> {
                    Path rel = Paths.get(dataDir).relativize(p);
                    String key = rel.toString().replace('\\', '/');
                    Class<?> clazz = registry.get(key);
                    if (clazz == null) return;
                    if (ITpl.class.isAssignableFrom(clazz)) {
                        loadTable(key, (Class<? extends ITpl>) clazz);
                    } else if (IConstTpl.class.isAssignableFrom(clazz)) {
                        loadConst(key, (Class<? extends IConstTpl>) clazz);
                    }
                });
        } catch (Exception e) { throw new RuntimeException("Failed to scan " + dataDir, e); }
    }

    public static <T extends ITpl> T get(Class<T> clazz, int id) {
        Map<Integer, ? extends ITpl> map = tables.get(clazz);
        return map != null ? (T) map.get(id) : null;
    }

    public static <T extends ITpl> Map<Integer, T> getAll(Class<T> clazz) {
        return (Map<Integer, T>) tables.get(clazz);
    }

    public static <T extends IConstTpl> T getConst(Class<T> clazz) {
        return (T) constants.get(clazz);
    }

    private static <T extends ITpl> void loadTable(String path, Class<T> clazz) {
        try {
            String json = new String(Files.readAllBytes(Paths.get(dataDir, path)));
            List<Map<String, Object>> list = parser.parseArray(json);
            Map<Integer, T> map = new HashMap<>();
            for (Map<String, Object> item : list) {
                T obj = fromMap(clazz, item);
                map.put(obj.getId(), obj);
            }
            tables.put(clazz, map);
        } catch (Exception e) { throw new RuntimeException("Failed to load " + path, e); }
    }

    private static <T extends IConstTpl> void loadConst(String path, Class<T> clazz) {
        try {
            String json = new String(Files.readAllBytes(Paths.get(dataDir, path)));
            Map<String, Object> map = parser.parseObject(json);
            T obj = fromMap(clazz, map);
            constants.put(clazz, obj);
        } catch (Exception e) { throw new RuntimeException("Failed to load " + path, e); }
    }

    private static <T> T fromMap(Class<T> clazz, Map<String, Object> map) {
        try {
            T obj = clazz.getDeclaredConstructor().newInstance();
            for (var field : clazz.getDeclaredFields()) {
                field.setAccessible(true);
                Object val = map.get(field.getName());
                if (val == null) {
                    field.set(obj, TplUtil.defaultValue(field.getType()));
                } else {
                    field.set(obj, TplUtil.convert(val, field.getType()));
                }
            }
            return obj;
        } catch (Exception e) { throw new RuntimeException(e); }
    }
}
