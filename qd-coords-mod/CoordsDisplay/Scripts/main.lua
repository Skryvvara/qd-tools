--[[
  CoordsDisplay - shows the player's world coordinates on screen in Queen's Domain.

  Requires UE4SS (https://github.com/UE4SS-RE/RE-UE4SS).

  Keys (change below):
    F6  toggle the overlay
    F7  log the current position to the UE4SS console and to positions.txt

  The numbers are Unreal world units (X / Y / Z), the same values listed in the
  qd-save-report HTML, so you can walk towards a missing collectible.
]]

local Config = {
    ToggleKey      = Key.F6,
    LogKey         = Key.F7,
    UpdateMs       = 100,      -- overlay refresh interval
    PosX           = 24,       -- overlay position in viewport pixels
    PosY           = 140,
    FontSize       = 20,
    ShowRotation   = true,
    ShowMap        = true,     -- second line: sublevel code + area, e.g. "500_AUT  (Ancient World)"
    StartVisible   = true,
}

local UEHelpers = require("UEHelpers")

local Visible = Config.StartVisible
local Widget = nil
local TextBlock = nil
local WidgetFailed = false
local LogFile = "ue4ss/Mods/CoordsDisplay/positions.txt"

local function Log(msg)
    print("[CoordsDisplay] " .. msg .. "\n")
end

local function GetPawn()
    local ok, pawn = pcall(function() return UEHelpers.GetPlayer() end)
    if ok and pawn and pawn:IsValid() then return pawn end
    pawn = FindFirstOf("FS_PlayerChar_C")
    if pawn and pawn:IsValid() then return pawn end
    return nil
end

-- Friendly names, same table as qd-save-report's report.rs
local AreaByMap = {
    ["140_GR_ShopIsleOnly"] = "Umbada's Islet",
    ["760_Dekanima"]        = "Dekanima (off Record of Sin)",
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

-- Name of the streamed sublevel (e.g. "500_AUT") an actor was placed in:
-- Actor -> outer ULevel "PersistentLevel" -> outer UWorld "500_AUT".
local function LevelOfActor(actor)
    local level = actor:GetOuter()
    if not level or not level:IsValid() then return nil end
    local world = level:GetOuter()
    if not world or not world:IsValid() then return nil end
    return world:GetFName():ToString()
end

-- The player itself lives in the persistent level, so look at what the player
-- is standing on instead. Remembered while jumping / falling.
local LastMap = nil
local function CurrentMap(pawn)
    local ok, map = pcall(function()
        local base = pawn:GetMovementBase()
        if not base or not base:IsValid() then return nil end
        local owner = base:GetOwner()
        if not owner or not owner:IsValid() then return nil end
        return LevelOfActor(owner)
    end)
    if ok and map and map ~= "" then LastMap = map end
    return LastMap
end

local function FormatPosition(pawn)
    local loc = pawn:K2_GetActorLocation()
    local s = string.format("X %.0f   Y %.0f   Z %.0f", loc.X, loc.Y, loc.Z)
    if Config.ShowRotation then
        local rot = pawn:K2_GetActorRotation()
        s = s .. string.format("   Yaw %.0f", rot.Yaw)
    end
    if Config.ShowMap then
        local map = CurrentMap(pawn)
        if map then
            local area = AreaName(map)
            s = s .. "\n" .. map .. (area ~= "" and ("  (" .. area .. ")") or "")
        end
    end
    return s, loc
end

-- Builds a UserWidget -> CanvasPanel -> TextBlock at runtime (no assets needed).
local function CreateOverlay()
    if WidgetFailed then return false end
    local ok, err = pcall(function()
        local gi = FindFirstOf("GameInstance")
        if not gi or not gi:IsValid() then error("no GameInstance yet") end

        local userWidgetClass = StaticFindObject("/Script/UMG.UserWidget")
        local treeClass       = StaticFindObject("/Script/UMG.WidgetTree")
        local canvasClass     = StaticFindObject("/Script/UMG.CanvasPanel")
        local textClass       = StaticFindObject("/Script/UMG.TextBlock")

        local w = StaticConstructObject(userWidgetClass, gi, FName("CoordsDisplayWidget"))
        w.WidgetTree = StaticConstructObject(treeClass, w, FName("CoordsDisplayTree"))
        local canvas = StaticConstructObject(canvasClass, w.WidgetTree, FName("CoordsDisplayCanvas"))
        w.WidgetTree.RootWidget = canvas

        local text = StaticConstructObject(textClass, w.WidgetTree, FName("CoordsDisplayText"))
        local slot = canvas:AddChildToCanvas(text)
        slot:SetAutoSize(true)
        slot:SetPosition({ X = Config.PosX, Y = Config.PosY })

        pcall(function()
            text:SetShadowOffset({ X = 1.5, Y = 1.5 })
            text:SetShadowColorAndOpacity({ R = 0, G = 0, B = 0, A = 0.9 })
        end)
        pcall(function()
            local font = text.Font
            font.Size = Config.FontSize
            text:SetFont(font)
        end)

        text:SetText(FText("..."))
        w:AddToViewport(1000)

        Widget, TextBlock = w, text
    end)
    if not ok then
        Log("could not create overlay: " .. tostring(err))
        -- GameInstance may simply not exist yet on the title screen; retry later
        if not tostring(err):find("no GameInstance") then WidgetFailed = true end
        return false
    end
    Log("overlay created")
    return true
end

local function OverlayAlive()
    if not Widget or not Widget:IsValid() then return false end
    local ok, inViewport = pcall(function() return Widget:IsInViewport() end)
    return ok and inViewport
end

local function SetOverlayVisible(v)
    if Widget and Widget:IsValid() then
        -- ESlateVisibility: 0 Visible, 1 Collapsed, 2 Hidden, 3 HitTestInvisible, 4 SelfHitTestInvisible
        pcall(function() Widget:SetVisibility(v and 3 or 1) end)
    end
end

local function Update()
    if not Visible then return end
    local pawn = GetPawn()
    if not pawn then return end
    if not OverlayAlive() then
        Widget, TextBlock = nil, nil
        if not CreateOverlay() then return end
        SetOverlayVisible(Visible)
    end
    local text = FormatPosition(pawn)
    pcall(function() TextBlock:SetText(FText(text)) end)
end

LoopAsync(Config.UpdateMs, function()
    ExecuteInGameThread(Update)
    return false -- keep looping
end)

RegisterKeyBind(Config.ToggleKey, function()
    ExecuteInGameThread(function()
        Visible = not Visible
        SetOverlayVisible(Visible)
        Log(Visible and "overlay on" or "overlay off")
    end)
end)

RegisterKeyBind(Config.LogKey, function()
    ExecuteInGameThread(function()
        local pawn = GetPawn()
        if not pawn then
            Log("no player pawn")
            return
        end
        local text = FormatPosition(pawn):gsub("\n", "   ")
        Log(text)
        local f = io.open(LogFile, "a") or io.open("Mods/CoordsDisplay/positions.txt", "a")
            or io.open("CoordsDisplay_positions.txt", "a")
        if f then
            f:write(os.date("%Y-%m-%d %H:%M:%S") .. "  " .. text .. "\n")
            f:close()
        end
    end)
end)

Log("loaded - F6 toggles the overlay, F7 logs your position")
