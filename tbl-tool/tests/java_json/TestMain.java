import com.game.config.*;
import com.game.config.hero.*;
import com.game.config.global.*;
import java.util.Map;

public class TestMain {
    public static void main(String[] args) {
        String dataDir = args.length > 0 ? args[0] : "gen/server/data";
        TplHolder.init(dataDir);

        System.out.println("=== HeroBase ===");
        Map<Integer, HeroBaseTpl> heroes = TplHolder.getAll(HeroBaseTpl.class);
        for (var entry : heroes.entrySet()) {
            var h = entry.getValue();
            System.out.printf("id=%d name=%s hp=%d skills=%s%n",
                h.getId(), h.getName(), h.getHp(), h.getSkills());
        }

        System.out.println("=== GlobalConst ===");
        var gc = TplHolder.getConst(GlobalConstTpl.class);
        System.out.printf("maxLevel=%d serverName=%s startPos=%s%n",
            gc.getMaxLevel(), gc.getServerName(), gc.getStartPos());
    }
}
