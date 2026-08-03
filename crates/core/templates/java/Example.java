package {{PACKAGE}};

/**
 * TplHolder 使用示例
 * <p>
 * 此类展示了如何初始化和使用配置表系统。
 * </p>
 */
public class Example {
    public static void main(String[] args) {
        // 方式1：从 XML 文件加载（推荐用于生产环境）
        // TplHolder.initXml("data/xml");

        // 方式2：从 JSON 文件加载（推荐用于开发/调试）
        TplHolder.initJson("data/json");

        // 方式3：自定义数据源（高级用法）
        // TblDataSource customSource = new MongoTblDataSource("mongodb://localhost/game");
        // TplHolder.init(customSource, TplRegistry.getRegistry());

        // 获取常量配置
        // GameConstTpl gameConst = TplHolder.getConst(GameConstTpl.class);
        // System.out.println("Game version: " + gameConst.version);

        // 获取表配置（通过主键）
        // ItemTpl item = TplHolder.get(ItemTpl.class, 1);
        // System.out.println("Item name: " + item.name);

        // 获取表的所有配置
        // Map<Integer, ItemTpl> allItems = TplHolder.getTable(ItemTpl.class);
        // allItems.values().forEach(item -> System.out.println(item.name));

        // 热更新单个表
        // TplHolder.reload("Item");

        // 批量热更新（原子替换）
        // TplHolder.reloadConfigs(Arrays.asList("Item", "HeroBase"));

        System.out.println("TplHolder example completed.");
    }
}
