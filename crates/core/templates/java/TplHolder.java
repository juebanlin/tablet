package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

@SuppressWarnings("unchecked")
public class TplHolder {
    private static volatile Map<Class<?>, Map<Integer, ? extends ITpl>> tables = new HashMap<>();
    private static final Map<Class<?>, IConstTpl> constants = new HashMap<>();

    // Parser 配置：外部可直接修改静态变量，或通过 setter 在 init 前设置
    private static IJsonParser jsonParser = new SimpleJsonParser();
    private static SimpleXmlParser xmlParser = new SimpleXmlParser();

    private static TblDataSource dataSource;
    private static String dataExt;

    // 配置表注册表：key → Class 映射（init 时注入，解耦 TplRegistry）
    private static Map<String, Class<?>> registry = new HashMap<>();
    // 反向映射：Class → key（用于查询表名）
    private static final Map<Class<?>, String> classToKey = new HashMap<>();

    // 字段匹配验证策略：默认宽松模式（忽略不匹配）
    private static TblFieldMismatchHandler mismatchHandler = new TblFieldMismatchHandler.Lenient();

    /**
     * 设置 JSON 解析器（在 init 前调用）
     */
    public static void setJsonParser(IJsonParser parser) {
        jsonParser = parser;
    }

    /**
     * 设置 XML 解析器（在 init 前调用）
     */
    public static void setXmlParser(SimpleXmlParser parser) {
        xmlParser = parser;
    }

    /**
     * 设置字段不匹配处理器（在 init 前调用）
     * @param handler 处理器实现（Strict/Lenient/LogToConsole 或自定义）
     */
    public static void setMismatchHandler(TblFieldMismatchHandler handler) {
        mismatchHandler = handler;
    }

    /**
     * 初始化（使用数据源抽象 + 注入注册表）
     * @param source 数据源实现
     * @param tableRegistry 配置表注册表（key → Class 映射）
     */
    public static void init(TblDataSource source, Map<String, Class<?>> tableRegistry) {
        dataSource = source;
        dataExt = source.getDataFormat();
        registry = new HashMap<>(tableRegistry);

        // 建立反向映射 Class → key
        classToKey.clear();
        for (Map.Entry<String, Class<?>> entry : tableRegistry.entrySet()) {
            classToKey.put(entry.getValue(), entry.getKey());
        }

        // 校验并加载所有配置
        validateAndLoadAll();
    }

    /**
     * 初始化（使用数据源抽象，从 TplRegistry 读取注册表）
     * @param source 数据源实现
     */
    public static void init(TblDataSource source) {
        init(source, TplRegistry.getAll());
    }

    /**
     * 初始化（使用数据源抽象 + 扫描指定包下的配置类）
     * @param source 数据源实现
     * @param packages 要扫描的包名（如 "com.game.config.tpl"）
     */
    public static void init(TblDataSource source, String... packages) {
        Map<String, Class<?>> scannedRegistry = new HashMap<>(TplRegistry.getAll());
        for (String pkg : packages) {
            scannedRegistry.putAll(TplRegistry.scanTblSourceClasses(pkg));
        }
        init(source, scannedRegistry);
    }

    /**
     * 校验配置完整性并批量加载
     */
    private static void validateAndLoadAll() {
        // 批量校验配置完整性
        dataSource.validateRequiredData(registry.keySet());

        // 批量加载
        loadAllFromSource(registry.keySet());
    }

    /**
     * 兼容方法：从文件系统加载 XML 格式配置
     * @param path 配置文件目录路径（支持相对路径和绝对路径）
     */
    public static void initXml(String path) {
        init(new FileSystemTblDataSource(path, ".xml"));
    }

    /**
     * 兼容方法：从文件系统加载 JSON 格式配置
     * @param path 配置文件目录路径（支持相对路径和绝对路径）
     */
    public static void initJson(String path) {
        init(new FileSystemTblDataSource(path, ".json"));
    }

    private static void loadAllFromSource(Set<String> keys) {
        Map<String, String> rawDataMap = dataSource.loadRawDataBatch(keys);
        for (Map.Entry<String, String> entry : rawDataMap.entrySet()) {
            String key = entry.getKey();
            String content = entry.getValue();
            if (content == null) continue;

            TblMeta meta = parseFromRaw(key, content);
            if (meta != null) apply(meta);
        }
    }

    private static TblMeta parseFromRaw(String key, String content) {
        Class<?> clazz = registry.get(key);
        if (clazz == null) return null;
        return parseFromRaw(key, clazz, content);
    }

    private static TblMeta parseFromRaw(String key, Class<?> clazz, String content) {
        try {
            String full = key + dataExt;
            SepConfig sep = parseSep(content, full);

            if (ITpl.class.isAssignableFrom(clazz)) {
                return TblMeta.table(key, clazz, sep, parseTableData(content, full));
            } else if (IConstTpl.class.isAssignableFrom(clazz)) {
                return TblMeta.constant(key, clazz, sep, parseConstData(content, full));
            }
            return null;
        } catch (Exception e) { throw new RuntimeException("Failed to parse " + key, e); }
    }

    public static void apply(TblMeta meta) {
        if (meta.isTable) {
            Map<Integer, ITpl> map = new HashMap<>();
            for (Map<String, String> item : meta.rows) {
                ITpl obj = (ITpl) fromMap(meta.key, meta.clazz, item, meta.sep);
                map.put(obj.getId(), obj);
            }
            tables.put(meta.clazz, map);
        } else {
            IConstTpl obj = (IConstTpl) fromMap(meta.key, meta.clazz, meta.constData, meta.sep);
            constants.put(meta.clazz, obj);
        }
    }

    /**
     * 单表热更新（直接 put，适合调试场景）
     */
    public static void reload(String key) {
        Class<?> clazz = registry.get(key);
        if (clazz == null) throw new IllegalArgumentException("Unknown registry key: " + key);

        String content = dataSource.loadRawData(key);
        if (content == null) throw new IllegalArgumentException("Data not found: " + key);

        TblMeta meta = parseFromRaw(key, content);
        if (meta == null) throw new IllegalStateException("Failed to parse: " + key);
        apply(meta);
    }

    /**
     * 批量热更新：原子替换缓存（Copy-on-Write）
     * @param keys 要更新的配置键列表
     * @return 旧缓存快照（用于回滚）
     */
    public static Map<Class<?>, Map<Integer, ? extends ITpl>> reloadConfigs(List<String> keys) {
        // 1. 复制当前缓存
        Map<Class<?>, Map<Integer, ? extends ITpl>> oldCache = tables;
        Map<Class<?>, Map<Integer, ? extends ITpl>> newCache = new HashMap<>(oldCache);

        // 2. 批量加载新数据到快照
        Map<String, String> rawDataMap = dataSource.loadRawDataBatch(keys);
        for (Map.Entry<String, String> entry : rawDataMap.entrySet()) {
            String key = entry.getKey();
            String content = entry.getValue();
            if (content == null) continue;

            TblMeta meta = parseFromRaw(key, content);
            if (meta == null) continue;

            if (meta.isTable) {
                Map<Integer, ITpl> map = new HashMap<>();
                for (Map<String, String> item : meta.rows) {
                    ITpl obj = (ITpl) fromMap(meta.key, meta.clazz, item, meta.sep);
                    map.put(obj.getId(), obj);
                }
                newCache.put(meta.clazz, map);
            } else {
                IConstTpl obj = (IConstTpl) fromMap(meta.key, meta.clazz, meta.constData, meta.sep);
                constants.put(meta.clazz, obj);  // 常量表单独缓存
            }
        }

        // 3. 原子替换（volatile 写）
        tables = newCache;

        // 4. 返回旧缓存（用于回滚）
        return oldCache;
    }

    /**
     * 恢复缓存（回滚批量更新）
     */
    public static void restoreCache(Map<Class<?>, Map<Integer, ? extends ITpl>> cache) {
        tables = cache;
    }

    /**
     * 获取当前数据源（供 GM 工具写入配置）
     */
    public static TblDataSource getDataSource() {
        return dataSource;
    }

    /**
     * 获取所有已注册的配置表名
     */
    public static Set<String> getRegisteredKeys() {
        return Collections.unmodifiableSet(registry.keySet());
    }

    /**
     * 根据表名查询对应的配置类
     * @param key 配置表名
     * @return 配置类，未找到返回 null
     */
    public static Class<?> getConfigClass(String key) {
        return registry.get(key);
    }

    /**
     * 根据配置类查询对应的表名
     * @param clazz 配置类
     * @return 表名，未找到返回 null
     */
    public static String getTableKey(Class<?> clazz) {
        return classToKey.get(clazz);
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

    private static SepConfig parseSep(String content, String path) {
        if (path.endsWith(".xml")) {
            return SepConfig.fromMap(xmlParser.parseRootAttrs(content));
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        return SepConfig.fromMap(toStringMap((Map<String, Object>) wrapper.getOrDefault("_sep", Collections.emptyMap())));
    }

    private static List<Map<String, String>> parseTableData(String content, String path) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseArray(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        List<Map<String, Object>> raw = (List<Map<String, Object>>) wrapper.get("data");
        return toStringMaps(raw);
    }

    private static Map<String, String> parseConstData(String content, String path) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseObject(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        Map<String, Object> raw = (Map<String, Object>) wrapper.getOrDefault("data", wrapper);
        return toStringMap(raw);
    }

    private static <T> T fromMap(String tableName, Class<T> clazz, Map<String, String> map, SepConfig sep) {
        try {
            T obj = clazz.getDeclaredConstructor().newInstance();

            // 收集配置类字段名
            Set<String> classFields = new HashSet<>();

            for (java.lang.reflect.Field field : clazz.getDeclaredFields()) {
                field.setAccessible(true);
                TblFieldType ann = field.getAnnotation(TblFieldType.class);
                if (ann == null) continue;

                classFields.add(field.getName());
                String raw = map.get(field.getName());

                // 验证：配置类字段在数据源中缺失
                if (raw == null && mismatchHandler != null) {
                    mismatchHandler.onClassFieldMissing(tableName, clazz.getSimpleName(), field.getName(), ann.value());
                }

                if (field.getType().isEnum() && ann.value().startsWith("@")) {
                    Object val = (raw == null || raw.isEmpty()) ? null
                        : field.getType().getMethod("fromId", int.class).invoke(null, Integer.parseInt(raw));
                    field.set(obj, val);
                } else {
                    field.set(obj, TplUtil.parseField(raw, ann.value(), sep));
                }
            }

            // 验证：数据源字段在配置类中不存在
            if (mismatchHandler != null) {
                for (Map.Entry<String, String> entry : map.entrySet()) {
                    String fieldName = entry.getKey();
                    if (!classFields.contains(fieldName)) {
                        mismatchHandler.onSourceExtraField(tableName, clazz.getSimpleName(), fieldName, entry.getValue());
                    }
                }
            }

            return obj;
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    private static List<Map<String, String>> toStringMaps(List<Map<String, Object>> raw) {
        List<Map<String, String>> result = new ArrayList<>();
        for (Map<String, Object> m : raw) {
            result.add(toStringMap(m));
        }
        return result;
    }

    private static Map<String, String> toStringMap(Map<String, Object> raw) {
        Map<String, String> result = new LinkedHashMap<>();
        for (Map.Entry<String, Object> e : raw.entrySet()) {
            result.put(e.getKey(), e.getValue() == null ? "" : e.getValue().toString());
        }
        return result;
    }
}