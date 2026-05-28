import com.game.config.*;
import com.game.config.hero.*;
import java.util.Map;

public class TestMain {
    public static void main(String[] args) {
        String dataDir = args.length > 0 ? args[0] : "gen/server/data";
        TplHolder.init(dataDir);

        System.out.println("=== HeroBase (omit strategy) ===");
        Map<Integer, HeroBaseTpl> heroes = TplHolder.getAll(HeroBaseTpl.class);
        for (var entry : heroes.entrySet()) {
            var h = entry.getValue();
            System.out.printf("id=%d name=%s hp=%d desc=[%s]%n",
                h.getId(), h.getName(), h.getHp(), h.getDesc());
        }
    }
}
