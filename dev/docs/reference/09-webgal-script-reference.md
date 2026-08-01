# WebGAL Script Reference

> Extracted from https://docs.openwebgal.com/ — reference for keine script engine design.

---

## 1. Script Basics

### File Structure
- Entry point: `scene/start.txt`
- All script files are `.txt` in `scene/` folder
- One statement per line, terminated with `;`
- Comments: everything after `;` on a line is ignored
- Escape special chars with `\`: `\:`, `\;`, `\,`, `\.`

### Generic Parameters (apply to all commands)

| Parameter | Type | Description |
|-----------|------|-------------|
| `-next` | bool | Execute next statement immediately (no wait for click) |
| `-when` | expr | Only execute if condition is true |
| `-continue` | bool | Auto-advance after this statement's animation completes |

---

## 2. Commands Reference

### dialogue — Say
```
Speaker:Hello world;
```
- Continuation: omit speaker for same character
- Narration: `:This is narration;`
- Voice: `Speaker:Hello -voice.wav;`
- `-notend`: dialogue not finished, more text/effects follow
- `-concat`: concatenate with previous dialogue (no click gap)
- `-vocal`: play voice file (also `-V`)
- `-volume`: voice volume 0-100

### intro — Fullscreen Text
```
intro:text part 1|part 2|part 3;
intro:text -hold;  // stay on screen until click
```

### changeBg — Background
```
changeBg:filename.jpg;
changeBg:none;     // remove background
changeBg:bg.jpg -next;  // immediate next
```

### changeFigure — Character Sprite
```
changeFigure:char.png;                    // center (default)
changeFigure:char.png -left;              // left position
changeFigure:char.png -right;             // right position
changeFigure:char.png -id=customId;       // free sprite with ID
changeFigure:none;                        // remove center
changeFigure:none -left;                   // remove left
changeFigure:none -id=customId;           // remove by ID
changeFigure:none -right;                  // remove right

// Mouth sync diffs
changeFigure:normal.png -id=charA
    -mouthOpen=open.png
    -mouthHalfOpen=half.png
    -mouthClose=close.png;

// With transform
changeFigure:stand.png -transform={"alpha":1,"position":{"x":0,"y":500},...} -next;
```

### miniAvatar — Portrait Thumbnail
```
miniAvatar:minipic.png;   // show
miniAvatar:none;           // hide
```

### bgm — Background Music
```
bgm:music.mp3;
bgm:none;                  // stop
bgm:music.mp3 -volume=30;
bgm:music.mp3 -enter=3000; // fade in over 3000ms
bgm:none -enter=3000;      // fade out
```

### playEffect — Sound Effect
```
playEffect:sfx.mp3;
playEffect:sfx.mp3 -volume=30;
playEffect:sfx.mp3 -id=loop1;   // looping
playEffect:none -id=loop1;       // stop loop
```

### playVideo — Video
```
playVideo:movie.webm;
playVideo:none;
```

### changeScene — Scene Transition
```
changeScene:Chapter-2.txt;
```

### callScene — Scene Call (subroutine, returns)
```
callScene:Chapter-2.txt;
```

### choose — Branch Selection
```
choose:Option A:sceneA.txt|Option B:sceneB.txt;
choose:Option A:label_a|Option B:label_b|Option C:label_c;

// Conditional display/enable
choose:(showCond>1)[enableCond>2]->Option:scene.txt|...;
```

### label / jumpLabel — Goto
```
label:myLabel;
jumpLabel:myLabel;
jumpLabel:myLabel -when=var>5;
```

### setVar — Variables
```
setVar:coin=10;
setVar:coin=coin+1;
setVar:name=(userInput);
setVar:lang=($userData.optionData.language);
```

### Variable Interpolation
```
WebGAL:You have {coin} coins.
{name}:I love WebGAL!
```

### if — Conditional (global -when)
```
changeScene:bad_end.txt -when=coin<0;
```

### setTextbox — Hide/Show Textbox
```
setTextbox:hide;    // hide
setTextbox:on;      // show (any non-hide value)
:;                   // shorthand hide, auto-restores next dialogue
```

### setAnimation — Play Animation
```
setAnimation:enter-from-bottom -target=fig-center -next;
setAnimation:shake -target=fig-left;
setAnimation:exit -target=bg-main;
setAnimation:blur -target=fig-right;
```

Predefined animations: `enter`, `exit`, `shake`, `enter-from-bottom/left/right`, `move-front-and-back`, `blur`, `oldFilm`, `dotFilm`, `reflectionFilm`, `glitchFilm`, `rgbFilm`, `godrayFilm`, `removeFilm`, `shockwaveIn`, `shockwaveOut`

Targets: `fig-left`, `fig-center`, `fig-right`, `bg-main`, or a custom `id`

### setTransform — Instant Transform
```
setTransform:{"position":{"x":100,"y":0}} -target=fig-center -duration=0;
```

### setTransition — Custom Enter/Exit Effects
```
setTransition: -target=fig-center -enter=enter-from-bottom -exit=exit;
```

### setComplexAnimation / setTempAnimation
```
setComplexAnimation:animName -target=fig-center;
```

Both commands share the same target-owned timeline as `setAnimation`. Built-in names use the
native presets; unknown names use the engine's bounded custom-animation fallback instead of
starting a web runtime.

### setFilter — Image-local GPU Filter

```text
setFilter:{"blur":6,"brightness":90,"contrast":110,"saturation":80} -target=fig-center;
setFilter:none -target=fig-center;
```

`brightness`, `contrast`, and `saturation` accept either ratios (`0.9`) or percentages (`90`).
Targets with no filter and alpha blending remain on the normal Sprite fast path.

### pixiInit / pixiPerform — PixiJS Effects
```
pixiInit:effect.json;
pixiPerform:effect.json;
```

keine maps these commands to a native, fixed-capacity Bevy effect layer. Names containing
`rain`, `snow`, `sakura`/`petal`, or `dust`/`light` select a preset; `pixiInit` clears the layer.

### wait — Delay
```
wait:1000;  // milliseconds
```

### end — Return to Title
```
end;
```

### filmMode — Film Mode
```
filmMode:enable;
filmMode:none;
```

### getUserInput — Text Input
```
getUserInput:name -title=What's your name? -buttonText=OK;
```

### comment — Script Comment
```
comment:This is a comment;
```

### applyStyle — UI Style
```
applyStyle:textbox.css;
```

### unlockCg / unlockBgm — Gallery Unlock
```
unlockCg:cg.jpg -name=Scene Name -series=1;
unlockBgm:song.mp3 -name=Song Name;
```

---

## 3. Custom Animation Format

Animations stored in `game/animation/` as JSON arrays. Registered in `animationTable.json`.

```json
[
  {
    "alpha": 0,
    "scale": {"x": 1, "y": 1},
    "position": {"x": -50, "y": 0},
    "rotation": 0,
    "blur": 5,
    "brightness": 1,
    "contrast": 1,
    "saturation": 1,
    "gamma": 1,
    "colorRed": 255,
    "colorGreen": 255,
    "colorBlue": 255,
    "oldFilm": 0,
    "dotFilm": 0,
    "reflectionFilm": 0,
    "glitchFilm": 0,
    "rgbFilm": 0,
    "godrayFilm": 0,
    "duration": 0
  },
  {
    "alpha": 1,
    "blur": 0,
    "position": {"x": 0, "y": 0},
    "duration": 500
  }
]
```

Keyframe at duration=0 is an instant transform. Duration is in milliseconds.

---

## 4. Game Config (config.txt)

| Key | Description |
|-----|-------------|
| `Game_name` | Game title |
| `Game_key` | Unique ID, 6-10 chars |
| `Title_img` | Title screen background |
| `Title_bgm` | Title screen music |
| `Game_Logo` | Logo images, `\|` separated |
| `Enable_Appreciation` | Enable CG/BGM gallery |
| `Default_Language` | zh_CN, zh_TW, en, ja, fr, de |
| `Show_panic` | Emergency hide feature |
| `Max_line` | Max dialogue lines |
| `Line_height` | Line height in em |
| `Steam_AppID` | Steam integration ID |

---

## 5. Text Enhancement Syntax

```
[text](style=color:#66327C; style-alltext=font-style:italic\;font-size:80%\; ruby=ruby text)
```

Params: `style` (text fill only), `style-alltext` (text + stroke + placeholder), `ruby` (furigana)

Note: `;` in CSS values must be escaped as `\;`

---

## 6. Ruby/Furigana

```
[word](reading)
Example: [笑顔](えがお)
```

---

## 7. Script Language Variables

Built-in: `$userData.optionData.language` (0=zh_CN, 1=en, 2=ja, 3=fr, 4=de, 5=zh_TW)

Custom variables set via `setVar` and interpolated via `{varName}`.

---

## 8. Resource Folders

| Folder | Content |
|--------|---------|
| `background/` | Background images |
| `figure/` | Character sprites |
| `bgm/` | Background music |
| `vocal/` | Voice files |
| `scene/` | Script .txt files |
| `animation/` | Custom animation JSON |
| `game/` | config.txt, animationTable.json |
