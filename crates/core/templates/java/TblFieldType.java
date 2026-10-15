package {{PACKAGE}};

import java.lang.annotation.*;

@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.FIELD)
public @interface TblFieldType {
    /** TblFieldType 字符串，反射加载时决定这个字段怎么解析。 */
    String value();

    /**
     * 数据文件里的字段 key（= .tbl 里的源字段名，见 @01.8.1）。
     *
     * 只在「源名 ≠ Java 字段名」时才生成：源名是 snake_case、Java 字段是 camelCase，
     * 两者的差正是需要标注的地方（`id` 这种两边相同的留空，回退到字段名）。
     */
    String name() default "";
}
