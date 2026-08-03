package {{PACKAGE}};

/**
 * 配置表字段不匹配处理器
 */
public interface TblFieldMismatchHandler {

    /**
     * 配置类字段在数据源中缺失
     * @param className 配置类名
     * @param fieldName 字段名
     * @param fieldType 字段类型
     */
    void onClassFieldMissing(String className, String fieldName, String fieldType);

    /**
     * 数据源字段在配置类中不存在
     * @param className 配置类名
     * @param extraField 多余字段名
     * @param sourceValue 数据源中的值
     */
    void onSourceExtraField(String className, String extraField, Object sourceValue);

    /**
     * 严格模式：任何不匹配都抛异常
     */
    class Strict implements TblFieldMismatchHandler {
        @Override
        public void onClassFieldMissing(String className, String fieldName, String fieldType) {
            throw new IllegalStateException(
                String.format("Field '%s' (%s) missing in data source for class %s",
                    fieldName, fieldType, className)
            );
        }

        @Override
        public void onSourceExtraField(String className, String extraField, Object sourceValue) {
            throw new IllegalStateException(
                String.format("Extra field '%s' in data source for class %s (value: %s)",
                    extraField, className, sourceValue)
            );
        }
    }

    /**
     * 宽松模式：忽略所有不匹配
     */
    class Lenient implements TblFieldMismatchHandler {
        @Override
        public void onClassFieldMissing(String className, String fieldName, String fieldType) {
            // 忽略
        }

        @Override
        public void onSourceExtraField(String className, String extraField, Object sourceValue) {
            // 忽略
        }
    }

    /**
     * 控制台输出模式：打印警告到 System.err
     */
    class LogToConsole implements TblFieldMismatchHandler {
        @Override
        public void onClassFieldMissing(String className, String fieldName, String fieldType) {
            System.err.printf("[WARN] Field '%s' (%s) missing in data source for class %s%n",
                fieldName, fieldType, className);
        }

        @Override
        public void onSourceExtraField(String className, String extraField, Object sourceValue) {
            System.err.printf("[WARN] Extra field '%s' in data source for class %s (value: %s)%n",
                extraField, className, sourceValue);
        }
    }
}
