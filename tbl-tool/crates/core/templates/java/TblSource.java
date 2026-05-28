package {{PACKAGE}};

import java.lang.annotation.*;

@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface TblSource {
    String group();
    String name();
    String mode();
}
