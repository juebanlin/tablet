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

    // 加载日志接口：默认控制台输出
    private static TblLoadLogger logger = new TblLoadLogger.Console();

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
     * 设置加载日志接口（在 init 前调用）
     * @param loggerImpl 日志实现（Console/Verbose/Silent 或自定义）
     */
    public static void setLogger(TblLoadLogger loggerImpl) {
        logger = loggerImpl;
    }

    /**
     * 初始化（使用数据源抽象 + 注入注册表）
     * @param source 数据源实现
     * @param tableRegistry 配置表注册表（key → Class 映射）
     */
    public static void init(TblDataSource source, Map<String, Class<?>> tableRegistry) {
        long startTime = System.currentTimeMillis();
        logger.info("Initializing with " + source.getClass().getSimpleName() +
                   " (format: " + source.getDataFormat() + ")");

        dataSource = source;
        dataExt = source.getDataFormat();
        registry = new HashMap<>(tableRegistry);

        // 建立反向映射 Class → key
        classToKey.clear();
        for (Map.Entry<String, Class<?>> entry : tableRegistry.entrySet()) {
            classToKey.put(entry.getValue(), entry.getKey());
        }

        logger.info("Registered " + registry.size() + " tables");

        // 校验并加载所有配置
        validateAndLoadAll();

        long elapsed = System.currentTimeMillis() - startTime;
        int totalRows = 0;
        for (Map<Integer, ? extends ITpl> map : tables.values()) {
            totalRows += map.size();
        }
        logger.info("Initialization completed: " + tables.size() + " tables, " +
                   constants.size() + " constants, " + totalRows + " total rows in " + elapsed + "ms");
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
        logger.debug("Scanning packages: " + String.join(", ", packages));
        Map<String, Class<?>> scannedRegistry = new HashMap<>(TplRegistry.getAll());
        for (String pkg : packages) {
            Map<String, Class<?>> found = TplRegistry.scanTblSourceClasses(pkg);
            logger.debug("Found " + found.size() + " classes in package " + pkg);
            scannedRegistry.putAll(found);
        }
        init(source, scannedRegistry);
    }

    /**
     * 校验配置完整性并批量加载
     */
    private static void validateAndLoadAll() {
        logger.info("Validating " + registry.size() + " registered tables...");
        // 批量校验配置完整性
        dataSource.validateRequiredData(registry.keySet());

        logger.info("Loading all tables...");
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
            if (content == null) {
                logger.warn("Skipping table '" + key + "': content is null");
                continue;
            }

            TblMeta meta = parseFromRaw(key, content);
            if (meta != null) {
                apply(meta);
                if (meta.isTable) {
                    Map<Integer, ? extends ITpl> loaded = tables.get(meta.clazz);
                    int rowCount = loaded != null ? loaded.size() : 0;
                    logger.debug("Loaded table '" + key + "' (" + rowCount + " rows)");
                } else {
                    logger.debug("Loaded constant '" + key + "'");
                }
            }
        }
    }

    private static TblMeta parseFromRaw(String key, String content) {
        Class<?> clazz = registry.get(key);
        if (clazz == null) return null;
        return parseFromRaw(key, clazz, content);
    }

    private static TblMeta parseFromRaw(String key, Class<?> clazz, String content) {
        return TplUtil.parseFromRaw(key, clazz, content, dataExt, xmlParser, jsonParser);
    }

    public static void apply(TblMeta meta) {
        if (meta.isTable) {
            Map<Integer, ITpl> map = new HashMap<>();
            for (Map<String, String> item : meta.rows) {
                ITpl obj = (ITpl) TplUtil.fromMap(meta.key, meta.clazz, item, meta.sep, mismatchHandler);
                map.put(obj.getId(), obj);
            }
            tables.put(meta.clazz, map);
        } else {
            IConstTpl obj = (IConstTpl) TplUtil.fromMap(meta.key, meta.clazz, meta.constData, meta.sep, mismatchHandler);
            constants.put(meta.clazz, obj);
        }
    }

    /**
     * 单表热更新（直接 put，适合调试场景）
     */
    public static void reload(String key) {
        logger.info("Reloading table '" + key + "'...");
        Class<?> clazz = registry.get(key);
        if (clazz == null) throw new IllegalArgumentException("Unknown registry key: " + key);

        String content = dataSource.loadRawData(key);
        if (content == null) throw new IllegalArgumentException("Data not found: " + key);

        TblMeta meta = parseFromRaw(key, content);
        if (meta == null) throw new IllegalStateException("Failed to parse: " + key);
        apply(meta);

        if (meta.isTable) {
            Map<Integer, ? extends ITpl> loaded = tables.get(meta.clazz);
            int rowCount = loaded != null ? loaded.size() : 0;
            logger.info("Reloaded table '" + key + "' (" + rowCount + " rows)");
        } else {
            logger.info("Reloaded constant '" + key + "'");
        }
    }

    /**
     * 批量热更新：原子替换缓存（Copy-on-Write）
     * @param keys 要更新的配置键列表
     * @return 旧缓存快照（用于回滚）
     */
    public static Map<Class<?>, Map<Integer, ? extends ITpl>> reloadConfigs(List<String> keys) {
        long startTime = System.currentTimeMillis();
        logger.info("Batch reloading " + keys.size() + " tables: " + keys);

        // 1. 复制当前缓存
        Map<Class<?>, Map<Integer, ? extends ITpl>> oldCache = tables;
        Map<Class<?>, Map<Integer, ? extends ITpl>> newCache = new HashMap<>(oldCache);

        // 2. 批量加载新数据到快照
        Map<String, String> rawDataMap = dataSource.loadRawDataBatch(keys);
        int reloadedCount = 0;
        for (Map.Entry<String, String> entry : rawDataMap.entrySet()) {
            String key = entry.getKey();
            String content = entry.getValue();
            if (content == null) {
                logger.warn("Skipping table '" + key + "': content is null");
                continue;
            }

            TblMeta meta = parseFromRaw(key, content);
            if (meta == null) {
                logger.warn("Failed to parse table '" + key + "'");
                continue;
            }

            if (meta.isTable) {
                Map<Integer, ITpl> map = new HashMap<>();
                for (Map<String, String> item : meta.rows) {
                    ITpl obj = (ITpl) TplUtil.fromMap(meta.key, meta.clazz, item, meta.sep, mismatchHandler);
                    map.put(obj.getId(), obj);
                }
                newCache.put(meta.clazz, map);
                logger.debug("Reloaded table '" + key + "' (" + map.size() + " rows)");
            } else {
                IConstTpl obj = (IConstTpl) TplUtil.fromMap(meta.key, meta.clazz, meta.constData, meta.sep, mismatchHandler);
                constants.put(meta.clazz, obj);  // 常量表单独缓存
                logger.debug("Reloaded constant '" + key + "'");
            }
            reloadedCount++;
        }

        // 3. 原子替换（volatile 写）
        tables = newCache;

        long elapsed = System.currentTimeMillis() - startTime;
        logger.info("Batch reload completed: " + reloadedCount + "/" + keys.size() + " tables in " + elapsed + "ms");

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
}