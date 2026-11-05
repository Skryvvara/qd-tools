--[[
  CoordsDisplay - shows the player's world coordinates on screen in Queen's Domain.

  Requires UE4SS (https://github.com/UE4SS-RE/RE-UE4SS).

  Keys (change below):
    F6  toggle the overlay
    F7  log the current position to the UE4SS console and to positions.txt

  The numbers are Unreal world units (X / Y / Z), the same values listed in the
  qd-save-report HTML, so you can walk towards a missing collectible.

  Layout (centred under the compass, every line centred, so changing numbers
  grow evenly to both sides instead of jumping):
        X -51407   Y 11692
            Z -30716
            500_AUT
         Ancient World

  The overlay hides itself while a menu is open (inventory, pause menu, travel,
  shop, ...) and whenever the game is paused.

  Look: the game's text style (DefaultTextStyle / BigTextStyle) is Rockwell
  Regular with the engine's pixelate effect and a 2 unit black drop shadow;
  the overlay uses exactly that.

  Placement: the game's HUD is a fixed 1920x1080 layout that is scaled to fit
  the screen and centred (UIAspectRatioBox). The compass sits in its top-right
  corner. The overlay computes that corner from the viewport size, so it stays
  right below the compass on every resolution and aspect ratio. It is a
  separate widget on purpose: the HUD is cached in an InvalidationBox, and
  changing text inside it forces the whole HUD to re-layout.

  Note: font settings can't be changed on a live TextBlock from Lua here
  (SetFont fails on this engine build), so everything is set before the widget
  is put on screen, and the widget is rebuilt when the resolution changes.
]]

local Config = {
    ToggleKey      = Key.F6,
    LogKey         = Key.F7,
    UpdateMs       = 200,   -- how often the numbers refresh
    GapBelow       = 4,     -- HUD units between the compass and the text
    TextScale      = 0.65,  -- relative to the item name in the lower left corner
    Pixelate       = true,  -- the game's pixelated text rendering
    ShowMap        = true,  -- last lines: sublevel code, then area name, e.g. "500_AUT" / "Ancient World"
    StartVisible   = true,
    HideWhenPaused = true,  -- also hide whenever the game is paused (photo mode, ...)
    -- Menus that hide the overlay while they are on screen.
    HideInMenus    = {
        "/Game/UI/Menus/GameMenu.GameMenu_C",                    -- inventory / equipment
        "/Game/UI/Menus/PauseMenu.PauseMenu_C",                  -- pause menu
        "/Game/UI/Menus/Journal/JournalMenu.JournalMenu_C",
        "/Game/UI/Menus/Journal/Guides_Menu.Guides_Menu_C",
        "/Game/UI/Menus/Challenges/Challenges_Menu.Challenges_Menu_C",
        "/Game/UI/Menus/OptionsMenu.OptionsMenu_C",
        "/Game/UI/Menus/Teleporter/TeleporterUI.TeleporterUI_C",       -- travel (wing stone)
        "/Game/UI/Menus/Teleporter/SaveTeleportUI.SaveTeleportUI_C",   -- travel from a save point
        "/Game/UI/Menus/SaveMenu.SaveMenu_C",
        "/Game/Blueprints/Shop/ShopMenu.ShopMenu_C",
        "/Game/UI/Menus/KeyUseMenu/KeyUseMenu.KeyUseMenu_C",
        "/Game/UI/Menus/ReadTextUI.ReadTextUI_C",
        "/Game/UI/Menus/DieUI.DieUI_C",
        "/Game/UI/DialogueSystem/DialogueScreen.DialogueScreen_C",
    },
}

-- Game HUD layout (FS_PlayerHUD / UIAspectRatioBox), in HUD units.
local HUD_W, HUD_H = 1920, 1080
-- Compass slot: 144 x 144, 30 in from the right edge and 30 down from the top.
local COMPASS_SIZE = 144
local COMPASS_CENTER_X = HUD_W - 30 - COMPASS_SIZE / 2
local COMPASS_BOTTOM = 30 + COMPASS_SIZE

-- Game text style (UI/UI_Style/FontStyles/DefaultTextStyle + BigTextStyle).
local FONT_PATH = "/Game/UI/UI_Style/FontStyles/Rockwell.Rockwell"
local FONT_TYPEFACE = "Regular"
local ITEM_FONT_SIZE = 26 * 0.75   -- BigTextStyle size, ConsumableHUD draws it at 0.75 scale
local TEXT_COLOR = 0.8             -- light grey
local SHADOW = 2                   -- drop shadow offset, HUD units
local PIXELATE = { bPixelate = true, PixelateMultiplier = 0.75, PixelateSharpen = 1.2, PixelateSharpenBias = 0.0 }

local SLOW_EVERY = math.max(1, math.floor(1000 / Config.UpdateMs)) -- ~1 s: lookups, layout

local Visible = Config.StartVisible
local Widget, TextBlock = nil, nil
local WidgetFailed = false
local BuiltFor = nil           -- layout key + font state the widget was built for
local FontLoaded = false
local Pawn = nil
local LastText = nil
local Shown = false            -- whether the overlay is currently on screen
local Tick = 0
local Created = 0              -- suffix for unique object names on re-creation
local LayoutLib, Statics = nil, nil
local Logged = {}              -- each distinct warning is logged once
local LogFile = "ue4ss/Mods/CoordsDisplay/positions.txt"

local function Log(msg)
    print("[CoordsDisplay] " .. msg .. "\n")
end

local function LogOnce(msg)
    if not Logged[msg] then
        Logged[msg] = true
        Log(msg)
    end
end

local function Valid(o)
    return o ~= nil and o:IsValid()
end

-- Cached; only searched again (a full object scan) when the pawn went away,
-- and at most once per slow tick.
local function GetPawn(allowSearch)
    if Valid(Pawn) then return Pawn end
    Pawn = nil
    if allowSearch then
        local p = FindFirstOf("FS_PlayerChar_C")
        if Valid(p) then Pawn = p end
    end
    return Pawn
end

---------------------------------------------------------------------------
-- Map line
---------------------------------------------------------------------------

-- Friendly names, same table as qd-save-report's report.rs
local AreaByMap = {
    ["140_GR_ShopIsleOnly"] = "Umbada's Islet",
    ["760_Dekanima"]        = "Dekanima",
    ["760_SZ_AUT"]          = "Record of Sin",
    ["740_SZ_Prison"]       = "Forgotten Dungeon",
    ["770_SZ_Swamp"]        = "Rotting Cavern",
    ["775_JigekiTemple"]    = "Jigeki's Temple",
    ["790_SG"]              = "Petrified Garden",
    ["800_FinalZone"]       = "Final Zone",
    ["900_SipehaWorld"]     = "Sipeha's World",
}
local AreaByPrefix = {
    ["0"] = "Isle of Neasied", ["1"] = "Isle of Neasied", ["2"] = "Obmuer Temple",
    ["3"] = "Tower of Knowledge", ["4"] = "Deimpo Mausoleum", ["5"] = "Ancient World",
    ["6"] = "Siraatsu Castle", ["7"] = "Underworld",
}

local function AreaName(map)
    return AreaByMap[map] or AreaByPrefix[map:sub(1, 1)] or ""
end

-- Sublevel (e.g. "500_AUT") of what the player stands on:
-- BasedMovement.MovementBase -> owner actor -> ULevel -> UWorld "500_AUT".
-- (Character:GetMovementBase() fails on this UE4SS/engine build, the
-- property works.) Remembered while jumping / falling.
local MapLine = nil

local function LookupMap(pawn)
    local trace = {}
    local function step(name, fn)
        local ok, v = pcall(fn)
        if not ok then
            table.insert(trace, name .. "=ERROR(" .. tostring(v) .. ")")
            LogOnce("map lookup failed at " .. name .. ": " .. tostring(v))
            return nil
        end
        if not Valid(v) then
            table.insert(trace, name .. "=none")
            return nil
        end
        local okn, n = pcall(function() return v:GetFullName() end)
        table.insert(trace, name .. "=" .. (okn and tostring(n) or "?"))
        return v
    end

    local base = step("base", function() return pawn.BasedMovement.MovementBase end)
    if not base then return nil, trace end
    local owner = step("owner", function() return base:GetOwner() end)
    if not owner then return nil, trace end
    local level = step("level", function() return owner:GetOuter() end)
    if not level then return nil, trace end
    local world = step("world", function() return level:GetOuter() end)
    if not world then return nil, trace end
    local ok, name = pcall(function() return world:GetFName():ToString() end)
    if not ok then
        table.insert(trace, "name=ERROR(" .. tostring(name) .. ")")
        return nil, trace
    end
    return name, trace
end

local function RefreshMap(pawn)
    local map = LookupMap(pawn)
    if map and map ~= "" then
        local area = AreaName(map)
        MapLine = map .. (area ~= "" and ("\n" .. area) or "")
    end
end

local function FormatPosition(pawn)
    local loc = pawn:K2_GetActorLocation()
    local s = string.format("X %.0f   Y %.0f\nZ %.0f", loc.X, loc.Y, loc.Z)
    if Config.ShowMap and MapLine then s = s .. "\n" .. MapLine end
    return s
end

---------------------------------------------------------------------------
-- Layout
---------------------------------------------------------------------------

-- Viewport size in pixels and the DPI scale of our (viewport) widget.
local function ViewportLayout(pawn)
    local ok, res = pcall(function()
        LayoutLib = LayoutLib or StaticFindObject("/Script/UMG.Default__WidgetLayoutLibrary")
        local size = LayoutLib:GetViewportSize(pawn)
        local W, H = size.X, size.Y
        if W < 1 or H < 1 then return nil end
        local dpi = LayoutLib:GetViewportScale(pawn)
        if not dpi or dpi <= 0 then dpi = H / 1080 end

        -- HUD rectangle on screen (pixels): 1920x1080 scaled to fit, centred.
        local s = math.min(W / HUD_W, H / HUD_H)
        local left = (W - HUD_W * s) / 2
        local top = (H - HUD_H * s) / 2
        local u = s / dpi -- one HUD unit in our widget's Slate units
        return {
            key  = string.format("%d:%d:%.4f", W, H, dpi),
            x    = (left + COMPASS_CENTER_X * s) / dpi,
            y    = (top + (COMPASS_BOTTOM + Config.GapBelow) * s) / dpi,
            size = math.max(8, math.floor(ITEM_FONT_SIZE * Config.TextScale * u + 0.5)),
            shadow = SHADOW * u,
            width = COMPASS_SIZE * u,
        }
    end)
    if not ok then
        LogOnce("viewport size unavailable: " .. tostring(res))
        return nil
    end
    return res
end

---------------------------------------------------------------------------
-- Widget
---------------------------------------------------------------------------

-- The Rockwell font is only in memory once the game has loaded its HUD; try
-- to load it ourselves if it isn't there yet.
local function FindFont()
    local f = StaticFindObject(FONT_PATH)
    if Valid(f) then return f end
    if LoadAsset then
        pcall(LoadAsset, FONT_PATH)
        f = StaticFindObject(FONT_PATH)
        if Valid(f) then return f end
    end
    return nil
end

-- Written straight into the TextBlock's properties before AddToViewport:
-- the Slate widget picks them up when it is built. (The setter functions
-- don't work for these structs on this build.)
local function StyleText(text, layout)
    local ok, err = pcall(function()
        local c = text.ColorAndOpacity
        c.SpecifiedColor.R = TEXT_COLOR
        c.SpecifiedColor.G = TEXT_COLOR
        c.SpecifiedColor.B = TEXT_COLOR
        c.SpecifiedColor.A = 1
        c.ColorUseRule = 0 -- UseColor_Specified
    end)
    if not ok then LogOnce("could not set text colour: " .. tostring(err)) end

    ok, err = pcall(function()
        local sc = text.ShadowColorAndOpacity
        sc.R, sc.G, sc.B, sc.A = 0, 0, 0, 1
        text.ShadowOffset.X = layout.shadow
        text.ShadowOffset.Y = layout.shadow
    end)
    if not ok then LogOnce("could not set text shadow: " .. tostring(err)) end

    FontLoaded = false
    ok, err = pcall(function()
        local font = text.Font
        local rockwell = FindFont()
        if rockwell then
            font.FontObject = rockwell
            font.TypefaceFontName = FName(FONT_TYPEFACE)
            FontLoaded = true
        end
        font.Size = layout.size
    end)
    if not ok then LogOnce("could not set font: " .. tostring(err)) end

    -- The game's pixelated text rendering (custom engine feature).
    if Config.Pixelate then ok, err = pcall(function()
        local p = text.Font.PixelateConfig
        p.bPixelate = PIXELATE.bPixelate
        p.PixelateMultiplier = PIXELATE.PixelateMultiplier
        p.PixelateSharpen = PIXELATE.PixelateSharpen
        p.PixelateSharpenBias = PIXELATE.PixelateSharpenBias
    end)
    if not ok then LogOnce("could not enable pixelated text: " .. tostring(err)) end end

    -- Lines centred on each other; the block itself is centred under the
    -- compass by the slot alignment.
    pcall(function() text.Justification = 1 end) -- ETextJustify::Center
    -- At least as wide as the compass, so short lines don't shrink the box.
    pcall(function() text.MinDesiredWidth = layout.width end)
end

local function DestroyOverlay()
    if Valid(Widget) then pcall(function() Widget:RemoveFromParent() end) end
    Widget, TextBlock, BuiltFor, LastText, Shown = nil, nil, nil, nil, false
end

-- UserWidget -> CanvasPanel -> TextBlock, built at runtime (no assets needed).
local function CreateOverlay(layout)
    if WidgetFailed then return false end
    local ok, err = pcall(function()
        local gi = FindFirstOf("GameInstance")
        if not Valid(gi) then error("no GameInstance yet") end
        Created = Created + 1
        local function N(base) return FName(base .. "_" .. Created) end

        local w = StaticConstructObject(StaticFindObject("/Script/UMG.UserWidget"), gi, N("CoordsDisplayWidget"))
        w.WidgetTree = StaticConstructObject(StaticFindObject("/Script/UMG.WidgetTree"), w, N("CoordsDisplayTree"))
        local canvas = StaticConstructObject(StaticFindObject("/Script/UMG.CanvasPanel"), w.WidgetTree, N("CoordsDisplayCanvas"))
        w.WidgetTree.RootWidget = canvas

        local text = StaticConstructObject(StaticFindObject("/Script/UMG.TextBlock"), w.WidgetTree, N("CoordsDisplayText"))
        StyleText(text, layout)

        local slot = canvas:AddChildToCanvas(text)
        slot:SetAutoSize(true)
        slot:SetAlignment({ X = 0.5, Y = 0 }) -- position = top-centre of the text
        slot:SetPosition({ X = layout.x, Y = layout.y })

        text:SetText(FText(""))
        w:AddToViewport(1000)
        w:SetVisibility(1) -- Collapsed until the update loop decides

        Widget, TextBlock = w, text
        BuiltFor = layout.key .. (FontLoaded and ":font" or "")
        LastText, Shown = nil, false
    end)
    if not ok then
        -- The GameInstance doesn't exist on the very first frames; retry later.
        if not tostring(err):find("no GameInstance") then
            Log("could not create overlay: " .. tostring(err))
            WidgetFailed = true
        end
        return false
    end
    Log("overlay created (font size " .. layout.size .. (FontLoaded and ", Rockwell" or ", default font") .. ")")
    return true
end

local function OverlayAlive()
    if not Valid(Widget) or not Valid(TextBlock) then return false end
    local ok, inVp = pcall(function() return Widget:IsInViewport() end)
    return ok and inVp
end

-- (Re)build the widget when it is missing, the resolution changed, or the
-- font has become available since it was built.
local function EnsureOverlay(pawn)
    local layout = ViewportLayout(pawn)
    if not layout then return OverlayAlive() end
    if OverlayAlive() then
        local wanted = layout.key .. ((FontLoaded or FindFont()) and ":font" or "")
        if wanted == BuiltFor then return true end
    end
    DestroyOverlay()
    return CreateOverlay(layout)
end

---------------------------------------------------------------------------
-- Menu detection
---------------------------------------------------------------------------

-- UE4SS tells us whenever one of the menu widgets is created, so per update
-- we only check the few known instances instead of scanning all objects. The
-- menu classes are loaded together with the player character, so the
-- watchers are registered once the player exists; classes that aren't loaded
-- yet are retried on later slow ticks.
local Menus = {}       -- known menu widget instances
local Watched = {}     -- class path -> true once NotifyOnNewObject succeeded

local function AddMenu(obj)
    for _, m in ipairs(Menus) do
        if m == obj then return end
    end
    table.insert(Menus, obj)
end

local function WatchMenus()
    for _, path in ipairs(Config.HideInMenus) do
        if not Watched[path] and Valid(StaticFindObject(path)) then
            local ok, err = pcall(function()
                NotifyOnNewObject(path, function(obj) AddMenu(obj) end)
            end)
            if ok then
                Watched[path] = true
                -- Instances that already exist (one-time scan).
                pcall(function()
                    local all = FindAllOf(path:match("%.([^.]+)$"))
                    if all then for _, m in ipairs(all) do AddMenu(m) end end
                end)
            else
                Watched[path] = true -- don't retry a failing registration
                LogOnce("can't watch " .. path .. ": " .. tostring(err))
            end
        end
    end
end

-- A menu counts as open when it is on screen, visible, and it or one of its
-- children has keyboard/gamepad focus. Some menus stay in the viewport while
-- closed, so being on screen alone isn't enough.
local function IsOpen(m)
    local ok, open = pcall(function()
        if not (m:IsInViewport() and m:IsVisible()) then return false end
        return m:HasAnyUserFocus() or m:HasFocusedDescendants()
    end)
    return ok and open
end

-- Returns the reason the overlay should be hidden, or nil.
local function HideReason(pawn)
    for i = #Menus, 1, -1 do
        local m = Menus[i]
        if not Valid(m) then
            table.remove(Menus, i)
        elseif IsOpen(m) then
            local ok, n = pcall(function() return m:GetFName():ToString() end)
            return "menu " .. (ok and n or "?")
        end
    end
    if Config.HideWhenPaused then
        local ok, paused = pcall(function()
            Statics = Statics or StaticFindObject("/Script/Engine.Default__GameplayStatics")
            return Statics:IsGamePaused(pawn)
        end)
        if ok and paused then return "game paused" end
    end
    return nil
end

local function SetShown(show, reason)
    if show == Shown or not Valid(Widget) then return end
    Shown = show
    Log(show and "shown" or ("hidden (" .. tostring(reason) .. ")"))
    pcall(function() Widget:SetVisibility(show and 3 or 1) end) -- HitTestInvisible / Collapsed
end

---------------------------------------------------------------------------
-- Main loop
---------------------------------------------------------------------------

local function Update()
    Tick = Tick + 1
    local slow = (Tick % SLOW_EVERY) == 1 or SLOW_EVERY == 1
    if not Visible then return end

    local pawn = GetPawn(slow)
    if not pawn then return end

    if slow then
        if not EnsureOverlay(pawn) then return end
        WatchMenus()
        RefreshMap(pawn)
    end
    if not TextBlock then return end

    local reason = HideReason(pawn)
    SetShown(reason == nil, reason)
    if reason then return end

    local ok, text = pcall(FormatPosition, pawn)
    if ok and text ~= LastText then
        LastText = text
        pcall(function() TextBlock:SetText(FText(text)) end)
    end
end

LoopAsync(Config.UpdateMs, function()
    ExecuteInGameThread(Update)
    return false -- keep looping
end)

RegisterKeyBind(Config.ToggleKey, function()
    ExecuteInGameThread(function()
        Visible = not Visible
        if not Visible then SetShown(false, "F6") end
        Tick = 0 -- next update runs the slow path
        Log(Visible and "overlay on" or "overlay off")
    end)
end)

RegisterKeyBind(Config.LogKey, function()
    ExecuteInGameThread(function()
        local pawn = GetPawn(true)
        if not pawn then
            Log("no player pawn")
            return
        end
        RefreshMap(pawn)
        local text = FormatPosition(pawn):gsub("\n", "   ")
        Log(text)
        local _, trace = LookupMap(pawn)
        Log("map lookup: " .. table.concat(trace, " | "))
        local f = io.open(LogFile, "a") or io.open("Mods/CoordsDisplay/positions.txt", "a")
            or io.open("CoordsDisplay_positions.txt", "a")
        if f then
            f:write(os.date("%Y-%m-%d %H:%M:%S") .. "  " .. text .. "\n")
            f:close()
        end
    end)
end)

Log("loaded - F6 toggles the overlay, F7 logs your position")
