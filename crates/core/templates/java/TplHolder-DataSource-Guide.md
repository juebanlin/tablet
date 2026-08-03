# TplHolder 数据源抽象 - 使用指南

## 概述

TplHolder 已重构为支持数据源抽象，参考配置加载指南的 ConfigSource 架构。核心改进：
- **数据源可切换**：文件系统 / Classpath / MongoDB / 自定义
- **注册表解耦**：支持自定义注册表注入，不绑死 TplRegistry
- **查询接口**：暴露已注册表名和 key ↔ Class 双向映射
- **100% 向后兼容**：旧代码无需修改

## 向后兼容

所有现有代码**无需修改**，旧 API 仍然有效：

```java
// 旧代码继续工作（内部使用 FileSystemTblDataSource + TplRegistry）
TplHolder.initXml("data/config");           // 原 init(String)，改名更明确
TplHolder.initJson("data/config");

// 自定义解析器（两种方式）
// 方式1：先设置后初始化（推荐）
TplHolder.setJsonParser(new MyJsonParser());
TplHolder.initJson("data/config");

// 方式2：一步到位（兼容旧代码）
TplHolder.initJson("data/config", new MyJsonParser());

// 查询 API 完全不变
Weapon weapon = TplHolder.get(Weapon.class, 1001);
Map<Integer, Hero> heroes = TplHolder.getAll(Hero.class);
GlobalConst consts = TplHolder.getConst(GlobalConst.class);
```

**注意**：`init(String)` 改名为 `initXml(String)` 使格式更明确。

## 核心改进

### 1. 注册表解耦（新增）

现在 TplHolder 不再绑死 TplRegistry，支持自定义注册表注入：

```java
// 场景A：使用 TplRegistry（默认，向后兼容）
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"));

// 场景B：自定义注册表（支持外部配置）
Map<String, Class<?>> customRegistry = new HashMap<>();
customRegistry.put("item/Weapon", Weapon.class);
customRegistry.put("hero/Hero", Hero.class);
customRegistry.put("external/Plugin", PluginConfig.class);  // 外部模块配置
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"), customRegistry);

// 场景C：运行时扫描 + 手动注入
Map<String, Class<?>> registry = new HashMap<>(TplRegistry.getAll());
// 扫描外部插件目录，加入更多配置类
registry.put("plugin/Quest", QuestConfig.class);
TplHolder.init(new MongoTblDataSource(collection, ".json"), registry);
```

### 2. 查询接口（新增）

暴露已注册的表名和双向映射：

```java
// 获取所有已注册的配置表名
Set<String> keys = TplHolder.getRegisteredKeys();
System.out.println("Registered tables: " + keys);

// key → Class 查询
Class<?> clazz = TplHolder.getConfigClass("item/Weapon");
if (clazz != null) {
    System.out.println("Weapon class: " + clazz.getName());
}

// Class → key 反向查询
String key = TplHolder.getTableKey(Weapon.class);
System.out.println("Weapon key: " + key);

// GM 工具：列出所有可推送的配置
for (String key : TplHolder.getRegisteredKeys()) {
    System.out.println("- " + key + " → " + TplHolder.getConfigClass(key).getSimpleName());
}
```

## Parser 配置

Parser 现在支持两种配置方式：

### 方式1：直接修改静态变量

```java
// 在 init 前修改（适合全局配置）
TplHolder.jsonParser = new GsonJsonParser();
TplHolder.xmlParser = new CustomXmlParser();
TplHolder.initJson("data/config");
```

### 方式2：使用 setter 方法

```java
// 在 init 前调用 setter（推荐，更清晰）
TplHolder.setJsonParser(new GsonJsonParser());
TplHolder.setXmlParser(new CustomXmlParser());
TplHolder.initJson("data/config");
```

## 数据源使用

### 1. 使用数据源抽象

```java
// 文件系统数据源（支持相对/绝对路径）
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"));

// Classpath 数据源（从 JAR 加载）
TplHolder.init(new ClasspathTblDataSource("/data/config", ".xml"));

// 混合使用：开发用文件，生产用 MongoDB
TblDataSource source = isDevelopment()
    ? new FileSystemTblDataSource("data/config", ".json")
    : new MongoTblDataSource(mongoCollection, ".json");
TplHolder.init(source);
```

### 2. GM 批量推送配置

```java
// 1. 获取数据源
TblDataSource source = TplHolder.getDataSource();

// 2. 批量写入配置
Map<String, String> configs = new HashMap<>();
configs.put("item/Weapon", newWeaponXml);
configs.put("hero/Hero", newHeroXml);
Map<String, Boolean> results = source.updateConfigs(configs);

// 3. 触发热更新（原子替换，Copy-on-Write）
Map<Class<?>, Map<Integer, ? extends ITpl>> oldCache = 
    TplHolder.reloadConfigs(Arrays.asList("item/Weapon", "hero/Hero"));

// 4. 如果失败，可以回滚
if (somethingWrong) {
    TplHolder.restoreCache(oldCache);
}
```

### 3. 单表热更新

```java
// 修改单个配置文件
source.updateConfig("item/Weapon", newXml);
TplHolder.reload("item/Weapon");  // 单表 put，无需原子替换
```

## 数据源接口

### TblDataSource 核心方法

```java
public interface TblDataSource {
    // === 必须实现 ===
    String loadRawData(String key);           // 加载单个配置
    Set<String> listAvailableKeys();          // 列出所有 key
    String getDataFormat();                   // ".xml" 或 ".json"
    
    // === 可选实现（写操作） ===
    void updateConfig(String key, String content);
    Map<String, Boolean> updateConfigs(Map<String, String> configs);
    boolean supportsWrite();                  // 默认 false
    
    // === 批量优化（default 实现，可覆写） ===
    Map<String, String> loadRawDataBatch(Collection<String> keys);
    void validateRequiredData(Set<String> requiredKeys);
}
```

### 内置实现

**FileSystemTblDataSource**：
- 从指定目录加载配置文件
- 支持读写操作
- 封装了原 TplHolder 的 Files.walk() 逻辑

**ClasspathTblDataSource**：
- 从 JAR 资源加载配置
- 只读数据源
- 适合打包后的生产环境

**MongoTblDataSource**（用户自己实现）：
- 需要 MongoDB 驱动依赖
- 可覆写批量优化方法（单次查询）

## 使用场景

### 场景 1：标准游戏服（文件系统）

```java
// 启动时初始化
TplHolder.initXml("data/config");

// 业务代码无需改变
Weapon weapon = TplHolder.get(Weapon.class, 1001);
```

### 场景 2：打包后的应用（Classpath）

```java
// 配置文件打包在 JAR 的 /data/config 目录下
TplHolder.init(new ClasspathTblDataSource("/data/config", ".xml"));
```

### 场景 3：GM 工具（管理多个游戏服）

```java
// GM 管理多个服的配置源
Map<String, TblDataSource> servers = new HashMap<>();
servers.put("server1", new FileSystemTblDataSource("D:/server1/data", ".xml"));
servers.put("server2", new MongoTblDataSource(mongo, ".json"));

// 推送到指定服
TblDataSource source = servers.get("server1");
source.updateConfig("item/Weapon", newXml);
// 通过 RPC 调用游戏服的热更新接口
```

### 场景 4：外部配置/插件扩展（注册表解耦）

```java
// 场景：游戏主包 + 外部插件包，插件有自己的配置类
// 插件配置类不在 TplRegistry 中（因为不是工具生成的）

// 1. 合并注册表
Map<String, Class<?>> registry = new HashMap<>(TplRegistry.getAll());
registry.put("plugin/Quest", QuestConfig.class);        // 外部插件
registry.put("plugin/Activity", ActivityConfig.class);  // 外部插件

// 2. 使用自定义注册表初始化
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"), registry);

// 3. 外部配置也能正常使用
QuestConfig quest = TplHolder.get(QuestConfig.class, 1001);
```

### 场景 5：运行时扫描配置类

```java
// 场景：多模块项目，配置类分散在不同包，希望启动时自动扫描

// 方式1：直接使用 init 的包扫描重载（推荐）
TplHolder.init(
    new FileSystemTblDataSource("data/config", ".xml"),
    "com.game.base.config.tpl",
    "com.game.battle.config.tpl",
    "com.game.social.config.tpl"
);

// 方式2：与 TplRegistry 混合使用
// init() 会自动合并 TplRegistry 和扫描结果
TplHolder.init(
    new FileSystemTblDataSource("data/config", ".xml"),
    "com.game.plugin.config.tpl"  // 仅追加插件包
);

// 3. 查询所有已注册的配置表
Set<String> keys = TplHolder.getRegisteredKeys();
System.out.println("Total tables: " + keys.size());
```

### 场景 6：GM 工具查询配置表

```java
// GM 工具需要列出所有可推送的配置表
public class GMConfigPanel {
    public void init() {
        Set<String> keys = TplHolder.getRegisteredKeys();
        for (String key : keys) {
            Class<?> clazz = TplHolder.getConfigClass(key);
            System.out.println("Table: " + key + " → " + clazz.getSimpleName());
        }
    }
    
    public void pushConfig(String key, String content) {
        // 1. 验证 key 存在
        if (TplHolder.getConfigClass(key) == null) {
            throw new IllegalArgumentException("Unknown table: " + key);
        }
        
        // 2. 推送到数据源
        TplDataSource source = TplHolder.getDataSource();
        source.updateConfig(key, content);
        
        // 3. 触发热更新
        TplHolder.reload(key);
    }
}
```

### 场景 7：动态环境切换

```java
String env = System.getProperty("env", "dev");
TblDataSource source = switch (env) {
    case "dev" -> new FileSystemTblDataSource("data/config", ".xml");
    case "test" -> new ClasspathTblDataSource("/data/config", ".xml");
    case "prod" -> new MongoTblDataSource(mongoCollection, ".json");
    default -> throw new IllegalArgumentException("Unknown env: " + env);
};
TplHolder.init(source);
```

## API 速查

| 方法 | 说明 |
|------|------|
| `initXml(String path)` | 兼容方法：XML + 文件系统 |
| `initJson(String path)` | 兼容方法：JSON + 文件系统 |
| `initJson(String path, IJsonParser parser)` | 兼容方法：JSON + 自定义解析器 |
| `init(TblDataSource source)` | 新方法：使用数据源抽象（从 TplRegistry 读取注册表） |
| `init(TblDataSource source, Map<String, Class<?>> registry)` | **新方法**：注入自定义注册表（解耦 TplRegistry） |
| `reload(String key)` | 单表热更新 |
| `reloadConfigs(List<String> keys)` | 批量原子热更新（Copy-on-Write） |
| `restoreCache(Map)` | 回滚批量更新 |
| `getDataSource()` | 获取当前数据源（供 GM 写入） |
| `getRegisteredKeys()` | **新方法**：获取所有已注册的配置表名 |
| `getConfigClass(String key)` | **新方法**：根据表名查询配置类 |
| `getTableKey(Class<?> clazz)` | **新方法**：根据配置类查询表名 |
| `get(Class, int)` | 查询单条配置（无变化） |
| `getAll(Class)` | 查询所有配置（无变化） |
| `getConst(Class)` | 查询常量配置（无变化） |

## 设计亮点

✅ **参考 ConfigSource 架构**：接口设计、批量优化、启动校验完全对齐  
✅ **注册表解耦**：不绑死 TplRegistry，支持自定义注册表注入  
✅ **暴露查询接口**：可查询已注册表名和 key ↔ Class 双向映射  
✅ **零外部依赖**：所有实现都是纯 JDK，模板文件保持零依赖  
✅ **100% 向后兼容**：旧代码不改一行照常运行  
✅ **灵活组合**：数据源 + 注册表 + 更新模式可任意组合  
✅ **启动即校验**：配置缺失在 init 阶段抛异常，不等运行时 NPE  
✅ **批量原子更新**：Copy-on-Write 机制，业务线程读到一致的缓存快照

## 迁移指南

### 如果你的代码使用了 `TplHolder.init(String)`

替换为：
```java
TplHolder.initXml(dataDir);  // 更明确的方法名
```

### 如果你需要支持 JAR 内加载

```java
// 开发时用文件系统
if (new File("data/config").exists()) {
    TplHolder.initXml("data/config");
} else {
    // 生产环境从 JAR 内加载
    TplHolder.init(new ClasspathTblDataSource("/data/config", ".xml"));
}
```

### 如果你需要实现自定义数据源（如 MongoDB）

```java
public class MongoTblDataSource implements TblDataSource {
    private final MongoCollection<Document> collection;
    private final String format;
    
    @Override
    public String loadRawData(String key) {
        Document doc = collection.find(eq("_id", key)).first();
        return doc != null ? doc.getString("content") : null;
    }
    
    @Override
    public Set<String> listAvailableKeys() {
        Set<String> keys = new LinkedHashSet<>();
        collection.find().projection(include("_id"))
            .forEach(doc -> keys.add(doc.getString("_id")));
        return keys;
    }
    
    @Override
    public String getDataFormat() {
        return format;
    }
    
    @Override
    public void updateConfig(String key, String content) {
        Document doc = new Document("_id", key).append("content", content);
        collection.replaceOne(eq("_id", key), doc, new ReplaceOptions().upsert(true));
    }
    
    @Override
    public boolean supportsWrite() {
        return true;
    }
}
```

## 常见问题

**Q: 旧代码需要改吗？**  
A: 不需要，`initXml()` 和 `initJson()` 完全向后兼容，查询 API 完全不变。

**Q: 为什么 `init(String)` 改名为 `initXml(String)`？**  
A: 使格式更明确，避免混淆。旧代码只需改方法名，参数和行为完全一致。

**Q: 如何扩展新的数据源？**  
A: 实现 `TblDataSource` 接口的 3 个核心方法即可，可选实现写操作和批量优化。

**Q: MongoDB 数据源需要额外依赖吗？**  
A: 需要，MongoTblDataSource 由用户项目自己实现（模板提供接口定义），需要引入 MongoDB 驱动。

**Q: 批量热更新和单表热更新有什么区别？**  
A: 批量热更新使用 Copy-on-Write 原子替换缓存，业务线程读到一致快照；单表热更新直接 put，适合调试场景。
