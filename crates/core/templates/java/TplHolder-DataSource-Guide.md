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

// 自定义解析器（在 init 前设置）
TplHolder.setJsonParser(new MyJsonParser());
TplHolder.setXmlParser(new MyXmlParser());
TplHolder.initJson("data/config");

// 查询 API 完全不变
Weapon weapon = TplHolder.get(Weapon.class, 1001);
Map<Integer, Hero> heroes = TplHolder.getAll(Hero.class);
GlobalConst consts = TplHolder.getConst(GlobalConst.class);
```

**注意**：`init(String)` 改名为 `initXml(String)` 使格式更明确。

## 核心改进

### 1. 格式校验（新增）

TplHolder 现在会自动校验配置文件内容格式是否与声明的格式匹配：

```java
// 示例：声明格式为 .xml，但实际内容是 JSON
TplHolder.initXml("data/config");  // 声明使用 XML 格式

// 如果 data/config/Item.xml 的内容是 {"items": [...]}，会抛出异常：
// IllegalStateException: Format mismatch for 'Item': expected XML (.xml) but content looks like JSON
```

**校验规则**：
- XML 格式：内容必须以 `<` 开头
- JSON 格式：内容必须以 `{` 或 `[` 开头
- 在 `loadAllFromSource()` 阶段对每个配置文件进行校验
- 校验失败会抛出 `IllegalStateException` 并中断启动

**好处**：
- **快速定位问题**：不再是模糊的解析异常，而是明确指出格式不匹配
- **避免人为错误**：防止 `.xml` 文件保存了 JSON 内容导致的运行时错误
- **启动时校验**：问题在 init 阶段就被发现，而不是运行时访问配置时才报错

### 2. 注册表解耦

现在 TplHolder 不再绑死 TplRegistry，支持自定义注册表注入：

```java
// 场景A：使用 TplRegistry（默认，向后兼容）
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"));

// 场景B：自定义注册表（支持外部配置）
Map<String, Class<?>> customRegistry = new HashMap<>();
customRegistry.put("Weapon", Weapon.class);
customRegistry.put("Hero", Hero.class);
customRegistry.put("Plugin", PluginConfig.class);  // 外部模块配置
TplHolder.init(new FileSystemTblDataSource("data/config", ".xml"), customRegistry);

// 场景C：运行时扫描指定包
TplHolder.init(
    new FileSystemTblDataSource("data/config", ".xml"),
    "com.game.config.tpl",
    "com.game.plugin.config.tpl"
);
```

### 3. 查询接口（新增）

暴露已注册的表名和双向映射：

```java
// 获取所有已注册的配置表名
Set<String> keys = TplHolder.getRegisteredKeys();
System.out.println("Registered tables: " + keys);

// key → Class 查询
Class<?> clazz = TplHolder.getConfigClass("Item");
if (clazz != null) {
    System.out.println("Item class: " + clazz.getName());
}

// Class → key 反向查询
String key = TplHolder.getTableKey(ItemTpl.class);
System.out.println("Item key: " + key);

// GM 工具：列出所有可推送的配置
for (String key : TplHolder.getRegisteredKeys()) {
    System.out.println("- " + key + " → " + TplHolder.getConfigClass(key).getSimpleName());
}
```

### 4. 热更新优化（新增）

新增了通过类自动获取表名的热更新方法，提供类型安全的 API：

```java
// 旧方式：需要手动输入表名字符串（容易拼错）
TplHolder.reload("Item");

// 新方式1：通过配置类自动获取表名（推荐，类型安全）
TplHolder.reloadTpl(ItemTpl.class);  // 自动从 @TblSource(name="Item") 获取

// 新方式2：常量表热更新
TplHolder.reloadConst(GameConstTpl.class);

// 批量原子更新（保持不变）
TplHolder.reloadConfigs(Arrays.asList("Item", "HeroBase"));
```

**优势**：
- **类型安全**：编译时检查类是否存在，IDE 支持自动补全和重构
- **避免拼写错误**：不需要手动输入表名字符串
- **自动获取**：从类的 `@TblSource(name="...")` 注解自动提取表名

### 5. 日志输出优化（新增）

日志输出更加精简和规范：

```
==================== TplHolder Initialization START ====================
Data Source: FileSystemTblDataSource (format: .xml)
Registered Tables: 5
Validating 5 registered tables...
Loading all tables...
loadTable: Item (42 rows) in 15ms
loadTable: HeroBase (28 rows) in 12ms
loadConst: GameConst in 3ms
Summary: 4 tables, 1 constants, 70 total rows in 45ms
==================== TplHolder Initialization END ====================
```

**日志级别规范**：
- **info 级别**：配置加载相关日志（启动、加载、热更新、统计）
- **warn 级别**：配置问题警告（内容为空、格式不匹配）
- **debug 级别**：详细的调试信息（包扫描、详细步骤）

**优化**：
- 数据源和格式信息合并为一行显示
- 不使用 debug 级别记录常规加载流程（配置加载属于关键操作）
- 精简日志输出，避免冗余信息

## Parser 配置

Parser 现在支持通过 setter 方法配置：

```java
// 在 init 前调用 setter（推荐）
TplHolder.setJsonParser(new GsonJsonParser());
TplHolder.setXmlParser(new CustomXmlParser());
TplHolder.initJson("data/config");

// 或者直接修改静态变量
TplHolder.jsonParser = new GsonJsonParser();
TplHolder.xmlParser = new CustomXmlParser();
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
configs.put("Item", newItemXml);
configs.put("HeroBase", newHeroXml);
Map<String, Boolean> results = source.updateConfigs(configs);

// 3. 触发热更新（原子替换，Copy-on-Write）
Map<Class<?>, Map<Integer, ? extends ITpl>> oldCache = 
    TplHolder.reloadConfigs(Arrays.asList("Item", "HeroBase"));

// 4. 如果失败，可以回滚
if (somethingWrong) {
    TplHolder.restoreCache(oldCache);
}
```

### 3. 单表热更新

```java
// 方式1：通过表名字符串
source.updateConfig("Item", newXml);
TplHolder.reload("Item");  // 单表 put，无需原子替换

// 方式2：通过类获取表名（类型安全，推荐）
source.updateConfig("Item", newXml);
TplHolder.reloadTpl(ItemTpl.class);  // 自动从 @TblSource 注解获取表名

// 方式3：常量表热更新
source.updateConfig("GameConst", newXml);
TplHolder.reloadConst(GameConstTpl.class);
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
registry.put("Quest", QuestConfig.class);        // 外部插件
registry.put("Activity", ActivityConfig.class);  // 外部插件

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
        TblDataSource source = TplHolder.getDataSource();
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
| `init(TblDataSource source)` | 新方法：使用数据源抽象（从 TplRegistry 读取注册表） |
| `init(TblDataSource source, Map<String, Class<?>> registry)` | **新方法**：注入自定义注册表（解耦 TplRegistry） |
| `init(TblDataSource source, String... packages)` | **新方法**：运行时扫描指定包下的配置类 |
| `reload(String key)` | 单表热更新（通过表名字符串） |
| `<T extends ITpl> reloadTpl(Class<T>)` | **新方法**：单表热更新（通过类自动获取 @TblSource 注解的表名） |
| `<T extends IConstTpl> reloadConst(Class<T>)` | **新方法**：常量表热更新（通过类自动获取 @TblSource 注解的表名） |
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
