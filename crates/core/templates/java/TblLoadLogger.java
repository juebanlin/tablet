package {{PACKAGE}};

/**
 * 配置表加载日志接口
 * <p>
 * 用于监控配置表的初始化、加载和热更新过程。
 * 默认使用 {@link Console} 实现输出到控制台，可替换为桥接到 SLF4J/Log4j2 等日志框架。
 * </p>
 */
public interface TblLoadLogger {
    /**
     * 记录信息级别日志
     */
    void info(String message);

    /**
     * 记录警告级别日志
     */
    void warn(String message);

    /**
     * 记录调试级别日志（默认实现通常不输出）
     */
    void debug(String message);

    /**
     * 控制台实现（默认）
     */
    class Console implements TblLoadLogger {
        @Override
        public void info(String message) {
            System.out.println("[TplHolder] " + message);
        }

        @Override
        public void warn(String message) {
            System.err.println("[TplHolder WARN] " + message);
        }

        @Override
        public void debug(String message) {
            // 默认不输出 debug，避免刷屏
        }
    }

    /**
     * 详细控制台实现（包含 debug 输出）
     */
    class Verbose implements TblLoadLogger {
        @Override
        public void info(String message) {
            System.out.println("[TplHolder] " + message);
        }

        @Override
        public void warn(String message) {
            System.err.println("[TplHolder WARN] " + message);
        }

        @Override
        public void debug(String message) {
            System.out.println("[TplHolder DEBUG] " + message);
        }
    }

    /**
     * 静默实现（无输出，用于测试或生产环境）
     */
    class Silent implements TblLoadLogger {
        @Override
        public void info(String message) {
        }

        @Override
        public void warn(String message) {
        }

        @Override
        public void debug(String message) {
        }
    }
}
