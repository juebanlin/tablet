package {{PACKAGE}}.types;

public class Tuple4<T1, T2, T3, T4> {
    public final T1 v1;
    public final T2 v2;
    public final T3 v3;
    public final T4 v4;

    public Tuple4(T1 v1, T2 v2, T3 v3, T4 v4) {
        this.v1 = v1;
        this.v2 = v2;
        this.v3 = v3;
        this.v4 = v4;
    }

    @Override
    public String toString() {
        return "(" + v1 + ", " + v2 + ", " + v3 + ", " + v4 + ")";
    }
}
