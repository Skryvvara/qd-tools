--[[
  CoordsDisplay - shows the player's world coordinates on screen in Queen's Domain.

  Requires UE4SS (https://github.com/UE4SS-RE/RE-UE4SS), tested with v4.0.0
  on game version 1.01.06a (Unreal Engine 4.27).

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

  Why the widget needs the right outer (issue #2): a UserWidget can only be put
  on screen when it can find its world. FindFirstOf("GameInstance") can return
  a stray plain UGameInstance that has no world; AddToViewport then silently
  does nothing and IsInViewport() stays false. The widget is therefore created
  under the game instance of the player's world (FS_GameInstance_C), owned by
  the player controller, and checked to be on screen after adding it.
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
    Debug          = false, -- log the widget's state on F7 (and once after it is first shown)
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

local VERSION = "1.1.0"

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

local WIDGET_PREFIX = "CoordsDisplayWidget"
local VIEWPORT_ZORDER = 1000

local SLOW_EVERY = math.max(1, math.floor(1000 / Config.UpdateMs)) -- ~1 s: lookups, layout
local RETRY_SLOW_TICKS = 10        -- wait ~10 s before retrying a widget that didn't reach the screen

-- Object names must be unique per load. UE4SS v4's FName() adds new names to
-- the name table (v3 returned NAME_None for unknown names, so the engine made
-- up a unique name). Re-using a name after a hot reload would make the engine
-- construct the new widget in place of the old one that is still on screen.
local SESSION = string.format("%04x%04x", os.time() % 0x10000, math.random(0, 0xFFFF))

local Visible = Config.StartVisible
local Widget, TextBlock = nil, nil
local WidgetFailed = false     -- creation failed in a way that won't fix itself
local RetryAt = nil            -- slow tick before which creation isn't retried
local BuiltKey = nil           -- layout key the widget was built for
local FontState = "none"       -- Rockwell: "none" (not in memory yet), "set", "failed"
local Pawn = nil
local LastText = nil
local Shown = false            -- whether the overlay is currently on screen
local Tick, SlowTick = 0, 0
local Created = 0              -- suffix for unique object names on re-creation
local LayoutLib, Statics = nil, nil
local LastLayout = nil
local AddedVia = nil           -- "viewport" or "player screen"
local Logged = {}              -- each distinct warning is logged once
local LogFile = "ue4ss/Mods/CoordsDisplay/positions.txt"
local StaleCleaned = false
local TriedLoadFont = false
local DiagDone = false

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

local function GetStatics()
    Statics = Statics or StaticFindObject("/Script/Engine.Default__GameplayStatics")
    return Statics
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
            key  = string.format("%.0f:%.0f:%.4f", W, H, dpi),
            W = W, H = H, dpi = dpi,
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
-- Diagnostics (Config.Debug)
---------------------------------------------------------------------------

local function Diag(tag)
    local parts = {}
    local function add(label, fn)
        local ok, v = pcall(fn)
        table.insert(parts, label .. "=" .. (ok and tostring(v) or ("ERR(" .. tostring(v) .. ")")))
    end
    local function name(o) return Valid(o) and o:GetFullName() or "none" end
    add("ver", function() return VERSION end)
    add("on", function() return Visible end)
    add("shown", function() return Shown end)
    add("addedVia", function() return AddedVia end)
    add("font", function() return FontState end)
    if LastLayout then
        add("layout", function()
            local l = LastLayout
            return string.format("%.0fx%.0f dpi %.3f at %.1f,%.1f size %d",
                l.W, l.H, l.dpi, l.x, l.y, l.size)
        end)
    end
    if Valid(Widget) then
        add("widget", function() return Widget:GetFullName() end)
        add("world", function() return name(Widget:GetWorld()) end)
        add("owner", function() return name(Widget:GetOwningPlayer()) end)
        add("inViewport", function() return Widget:IsInViewport() end)
        add("visibility", function() return Widget:GetVisibility() end)
    else
        add("widget", function() return "none" end)
    end
    if Valid(TextBlock) then
        add("text", function() return "'" .. TextBlock:GetText():ToString():gsub("\n", "/") .. "'" end)
        add("fontSize", function() return TextBlock.Font.Size end)
        add("fontObj", function() return name(TextBlock.Font.FontObject) end)
    end
    Log("diag[" .. tag .. "] " .. table.concat(parts, " | "))
end

---------------------------------------------------------------------------
-- Widget
---------------------------------------------------------------------------

-- The Rockwell font is only in memory once the game has loaded its HUD; try
-- to load it ourselves (once) if it isn't there yet.
local function FindFont()
    local f = StaticFindObject(FONT_PATH)
    if Valid(f) then return f end
    if LoadAsset and not TriedLoadFont then
        TriedLoadFont = true
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

    ok, err = pcall(function() text.Font.Size = layout.size end)
    if not ok then LogOnce("could not set font size: " .. tostring(err)) end

    local rockwell = FindFont()
    if rockwell then
        ok, err = pcall(function()
            local font = text.Font
            font.FontObject = rockwell
            font.TypefaceFontName = FName(FONT_TYPEFACE)
        end)
        FontState = ok and "set" or "failed"
        if not ok then LogOnce("could not set font: " .. tostring(err)) end
    else
        FontState = "none"
    end

    -- The game's pixelated text rendering (custom engine feature).
    if Config.Pixelate then
        ok, err = pcall(function()
            local p = text.Font.PixelateConfig
            p.bPixelate = PIXELATE.bPixelate
            p.PixelateMultiplier = PIXELATE.PixelateMultiplier
            p.PixelateSharpen = PIXELATE.PixelateSharpen
            p.PixelateSharpenBias = PIXELATE.PixelateSharpenBias
        end)
        if not ok then LogOnce("could not enable pixelated text: " .. tostring(err)) end
    end

    -- Lines centred on each other; the block itself is centred under the
    -- compass by the slot alignment.
    pcall(function() text.Justification = 1 end) -- ETextJustify::Center
    -- At least as wide as the compass, so short lines don't shrink the box.
    pcall(function() text.MinDesiredWidth = layout.width end)
end

local function DestroyOverlay()
    if Valid(Widget) then pcall(function() Widget:RemoveFromParent() end) end
    Widget, TextBlock, BuiltKey, LastText, Shown, AddedVia = nil, nil, nil, nil, false, nil
end

-- A hot reload throws away the Lua state but not the widget it put on
-- screen; take those leftovers off the screen before building a new one.
local function RemoveStaleWidgets()
    if StaleCleaned then return end
    StaleCleaned = true
    local ok, err = pcall(function()
        local all = FindAllOf("UserWidget")
        if not all then return end
        local n = 0
        for _, w in ipairs(all) do
            if Valid(w) then
                local okn, name = pcall(function() return w:GetFName():ToString() end)
                if okn and name:sub(1, #WIDGET_PREFIX) == WIDGET_PREFIX then
                    pcall(function() w:RemoveFromParent() end)
                    n = n + 1
                end
            end
        end
        if n > 0 then Log("removed " .. n .. " overlay widget(s) left over from an earlier load") end
    end)
    if not ok then LogOnce("cleanup of old widgets failed: " .. tostring(err)) end
end

local function InViewport(w)
    local ok, inVp = pcall(function() return w:IsInViewport() end)
    return ok and inVp == true
end

-- Errors starting with this are "the world isn't ready yet": retried quietly.
local NOT_READY = "not ready: "

-- UserWidget -> CanvasPanel -> TextBlock, built at runtime (no assets needed).
local function CreateOverlay(layout, pawn)
    if WidgetFailed then return false end
    if RetryAt and SlowTick < RetryAt then return false end
    RetryAt = nil
    RemoveStaleWidgets()
    local w = nil
    local ok, err = pcall(function()
        -- The game instance of the player's world (FS_GameInstance_C), not
        -- FindFirstOf("GameInstance"): see the note at the top.
        local gi = GetStatics():GetGameInstance(pawn)
        if not Valid(gi) then error(NOT_READY .. "no game instance") end
        local pc = pawn.Controller
        if not Valid(pc) then error(NOT_READY .. "no player controller") end

        Created = Created + 1
        local function N(base) return FName(base .. "_" .. SESSION .. "_" .. Created) end

        w = StaticConstructObject(StaticFindObject("/Script/UMG.UserWidget"), gi, N(WIDGET_PREFIX))
        w:SetOwningPlayer(pc)
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
        w:SetVisibility(1) -- Collapsed until the update loop decides

        -- AddToViewport does nothing (no error) when the widget can't find
        -- its world, so check that it really is on screen.
        w:AddToViewport(VIEWPORT_ZORDER)
        if InViewport(w) then
            AddedVia = "viewport"
        else
            w:AddToPlayerScreen(VIEWPORT_ZORDER)
            if InViewport(w) then AddedVia = "player screen" end
        end
        if not AddedVia then
            local okw, world = pcall(function() return w:GetWorld() end)
            error("the widget did not reach the screen (world: "
                .. ((okw and Valid(world)) and world:GetFullName() or "none") .. ")")
        end

        Widget, TextBlock = w, text
        BuiltKey = layout.key
        LastText, Shown = nil, false
    end)
    if not ok then
        local msg = tostring(err)
        if Valid(w) then pcall(function() w:RemoveFromParent() end) end
        if msg:find(NOT_READY, 1, true) then
            return false -- the first frames of a level; try again next slow tick
        end
        if msg:find("did not reach the screen", 1, true) then
            LogOnce("could not show overlay: " .. msg .. " - retrying every " .. RETRY_SLOW_TICKS .. " s")
            RetryAt = SlowTick + RETRY_SLOW_TICKS
            return false
        end
        Log("could not create overlay: " .. msg)
        WidgetFailed = true
        return false
    end
    LastLayout = layout
    Log(string.format("overlay created (%s, font size %d, %s, viewport %.0fx%.0f, dpi %.3f)",
        AddedVia, layout.size, FontState == "set" and "Rockwell" or ("default font, Rockwell " .. FontState),
        layout.W, layout.H, layout.dpi))
    return true
end

local function OverlayAlive()
    return Valid(Widget) and Valid(TextBlock) and InViewport(Widget)
end

-- (Re)build the widget when it is missing, the resolution changed, or the
-- font has become available since it was built.
local function EnsureOverlay(pawn)
    local layout = ViewportLayout(pawn)
    if not layout then return OverlayAlive() end
    if OverlayAlive() then
        local fontNowAvailable = FontState == "none" and FindFont() ~= nil
        if layout.key == BuiltKey and not fontNowAvailable then return true end
        Log("rebuilding overlay (" .. (layout.key ~= BuiltKey
            and ("viewport " .. tostring(BuiltKey) .. " -> " .. layout.key) or "font loaded") .. ")")
    elseif Widget then
        -- E.g. the engine removed it because the level was unloaded.
        Log("overlay left the screen, rebuilding")
    end
    DestroyOverlay()
    return CreateOverlay(layout, pawn)
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
        local ok, paused = pcall(function() return GetStatics():IsGamePaused(pawn) end)
        if ok and paused then return "game paused" end
    end
    return nil
end

local function SetShown(show, reason)
    if show == Shown or not Valid(Widget) then return end
    Shown = show
    Log(show and "shown" or ("hidden (" .. tostring(reason) .. ")"))
    local ok, err = pcall(function() Widget:SetVisibility(show and 3 or 1) end) -- HitTestInvisible / Collapsed
    if not ok then LogOnce("could not change visibility: " .. tostring(err)) end
end

---------------------------------------------------------------------------
-- Main loop
---------------------------------------------------------------------------

local function Update()
    Tick = Tick + 1
    local slow = (Tick % SLOW_EVERY) == 1 or SLOW_EVERY == 1
    if slow then SlowTick = SlowTick + 1 end
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
        local okSet, err = pcall(function() TextBlock:SetText(FText(text)) end)
        if not okSet then LogOnce("could not set text: " .. tostring(err)) end
        if Config.Debug and not DiagDone then
            DiagDone = true
            Diag("first text")
        end
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
        if Config.Debug then Diag("F7") end
        local f = io.open(LogFile, "a") or io.open("Mods/CoordsDisplay/positions.txt", "a")
            or io.open("CoordsDisplay_positions.txt", "a")
        if f then
            f:write(os.date("%Y-%m-%d %H:%M:%S") .. "  " .. text .. "\n")
            f:close()
        end
    end)
end)

Log("loaded v" .. VERSION .. " - F6 toggles the overlay, F7 logs your position")
