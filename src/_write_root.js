const fs = require("fs");
const content = `import "./index.css";
import { Composition } from "remotion";
import { MyComposition } from "./Composition";

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition
        id="MyComp"
        component={MyComposition}
        durationInFrames={420}
        fps={30}
        width={1280}
        height={720}
      />
    </>
  );
};
`;
const target = "C:\\Users\\Administrator\\Desktop\\临时\\pointer-intro\\src\\Root.tsx";
fs.writeFileSync(target, content, "utf8");
console.log("Written to " + target);
