package {{PACKAGE}};

import java.util.*;

/**
 * 数据源接口 - 统一抽象配置表的数据来源
 * 支持文件系统、Classpath、MongoDB 等多种实现
 */
public interface TblDataSource {

    // === 核心方法（必须实现） ===

    /**
     * 加载单个配置文件的原始内容
     * @param key 配置键（如 "item/Weapon"，不含扩展名）
     * @return 文件内容（UTF-8 字符串），不存在返回 null
     */
    String loadRawData(String key);

    /**
     * 列出数据源中所有可用的配置键
     * @return 配置键集合（如 ["item/Weapon", "hero/Hero"]）
     */
    Set<String> listAvailableKeys();

    /**
     * 获取数据格式（用于判断解析方式）
     * @return ".xml" 或 ".json"
     */
    String getDataFormat();

    // === 写操作方法（可选实现） ===

    /**
     * 更新单个配置文件内容
     * @param key 配置键
     * @param content 原始内容（XML/JSON 字符串）
     * @throws UnsupportedOperationException 如果数据源只读
     */
    default void updateConfig(String key, String content) {
        throw new UnsupportedOperationException("This data source is read-only");
    }

    /**
     * 批量更新配置文件（GM 推送场景）
     * @param configs key -> content 映射
     * @return key -> 成功/失败 映射
     */
    default Map<String, Boolean> updateConfigs(Map<String, String> configs) {
        Map<String, Boolean> results = new LinkedHashMap<>();
        for (Map.Entry<String, String> entry : configs.entrySet()) {
            try {
                updateConfig(entry.getKey(), entry.getValue());
                results.put(entry.getKey(), true);
            } catch (Exception e) {
                results.put(entry.getKey(), false);
            }
        }
        return results;
    }

    /**
     * 数据源是否支持写操作
     */
    default boolean supportsWrite() {
        return false;
    }

    // === 批量优化方法（default 实现，子类可覆写） ===

    /**
     * 批量加载配置文件（减少 I/O 或网络调用）
     * @param keys 配置键列表
     * @return key -> rawContent 映射（不存在的键返回 null 值）
     */
    default Map<String, String> loadRawDataBatch(Collection<String> keys) {
        Map<String, String> result = new LinkedHashMap<>();
        for (String key : keys) {
            result.put(key, loadRawData(key));
        }
        return result;
    }

    /**
     * 验证必需配置是否存在（启动时校验）
     * @param requiredKeys 业务层需要的配置键（从 TplRegistry 获取）
     * @throws IllegalStateException 如果有缺失的配置
     */
    default void validateRequiredData(Set<String> requiredKeys) {
        Set<String> available = listAvailableKeys();
        Set<String> missing = new LinkedHashSet<>();
        for (String key : requiredKeys) {
            if (!available.contains(key)) {
                missing.add(key);
            }
        }
        if (!missing.isEmpty()) {
            throw new IllegalStateException("Missing required config files: " + missing);
        }
    }
}
