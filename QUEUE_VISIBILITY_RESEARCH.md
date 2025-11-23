# Queue Count Visibility Options - Research

## Goal
Find a way to display queue count (e.g., "5/8") that's **always visible** when in the server, **without opening specific channels**.

---

## Options Comparison

### 1. ✅ **Voice Channel Topic** (RECOMMENDED)

**How it looks:**
- Hover over voice channel → see topic in tooltip
- Mobile: Tap channel name → see description
- Desktop: Always visible in channel details panel (when channel selected)

**Rate Limit:**
- **2 changes per 10 minutes** (same as channel name)
- Shares bucket with channel name OR separate? (Discord docs inconsistent)
- According to GitHub issue #2190: name and topic have separate buckets UNLESS used together

**Visibility:**
- **Not always visible** - requires hovering/clicking
- Better on mobile (shows in channel info)
- Less prominent than channel name

**Implementation:**
```rust
ctx.http.edit_channel(
    queue_vc,
    &EditChannel::new().topic(format!("Queue: {}/{}", count, quota)),
    Some("Update queue status")
).await
```

**Pros:**
- ✅ Separate rate limit bucket from channel name (probably)
- ✅ Can update BOTH topic AND name for redundancy
- ✅ No interference with channel naming conventions
- ✅ Clean, doesn't clutter channel list

**Cons:**
- OFFt always visible (need to hover/click)
- ❌ Still has 2/10min rate limit
- ❌ Unclear if it shares bucket with name changes

**Verdict:** 6/10 - Better than nothing, but not "always visible"

---

### 2. **Bot Server Nickname** (BEST OPTION)

**How it looks:**
- Bot appears in member list as "PUG Bot [5/8]"
- Always visible in right sidebar
- Prominent and easy to spot

**Rate Limit:**
- **Standard rate limit: ~1000 requests per day per guild**
- That's ~once per 86 seconds sustained
- For your use case (updates on join/leave): **PERFECT**
- Per-guild, so multi-server won't interfere

**Visibility:**
- ✅ **ALWAYS VISIBLE** in member list (right sidebar)
- ✅ Works on mobile and desktop
- ✅ Doesn't require any navigation
- ✅ Stands out due to bot badge/color

**Implementation:**
```rust
// Get bot's member object in guild
let bot_member = guild_id.member(&ctx.http, ctx.cache.current_user().id).await?;

// Update nickname
bot_member.edit(&ctx.http, EditMember::new().nickname(
    format!("PUG Bot [Queue: {}/{}]", count, quota)
)).await?;
```

**Pros:**
- ✅ ALWAYS visible in member list
- ✅ 1000/day limit = can update every join/leave easily
- ✅ Per-guild rate limit (multi-server safe)
- ✅ Prominent and impossible to miss
- ✅ No channel clutter
- ✅ Works everywhere (mobile, desktop, web)

**Cons:**
- ❌ Only shows ONE group's queue (problem if you have multiple groups per server)
- ❌ Takes space in member list
- ❌ Might be considered "unconventional" by some users

**Verdict:** 9/10 - **BEST for single-group servers**

---

### 3. ❌ **Category Name**

**How it looks:**
- Category: "PUG QUEUE [5/8]"
- Very prominent, always visible

**Rate Limit:**
- **2 changes per 10 minutes** (same as channel name)
- Categories are channels, same API endpoint

**Visibility:**
- ✅ **ALWAYS VISIBLE** in channel list
- ✅ Very prominent (larger text)
- ✅ Works on mobile and desktop

**Implementation:**
```rust
// Same as channel name editing
ctx.http.edit_channel(category_id, &EditChannel::new().name(...))
```

**Pros:**
- ✅ Extremely visible
- ✅ Always shown in channel list
- ✅ Can't be missed

**Cons:**
- ❌ **Same 2/10min rate limit as channel names**
- ❌ NO BENEFIT over channel name approach
- ❌ Wastes the rate limit on less flexible location
- ❌ Categories are supposed to be organizational, not dynamic

**Verdict:** 2/10 - Same problem as channel names, worse location

---

### 4. ❌ **Role Name** (e.g., "In Queue [5/8]")

**How it looks:**
- Role in server settings: "In Queue [5/8]"
- Members with role show this in member list

**Rate Limit:**
- **1000 changes per 24 hours per role**
- That's once per ~86 seconds sustained
- **Sounds good BUT...**

**Visibility:**
- Only visible if you:
  - Open role list in server settings, OR
  - Have role hoisted and visible in member list
  
**Implementation:**
```rust
// Edit role name
role_id.edit(&ctx.http, EditRole::new().name(
    format!("In Queue [5/8]")
)).await?;
```

**Pros:**
- ✅ 1000/day limit (reasonable)
- ✅ Could be hoisted for visibility

**Cons:**
- ❌ **Role names aren't prominently visible**
- ❌ Only shows in member list if hoisted (creates clutter)
- ❌ Users would see "In Queue [5/8]" next to their name (confusing)
- ❌ Role is meant to be assigned to players, not display data
- ❌ Semantic abuse of roles system

**Verdict:** 1/10 - Bad UX, confusing, not truly visible

---

### 5. **Hoisted Role with Count in Members** (ALTERNATIVE)

**How it looks:**
- Create role "Queue Status"
- Assign ONLY to bot
- Hoist it to top of member list
- Bot's nickname shows count
- Role section header shows "Queue Status" with bot underneath showing "[5/8]"

**Rate Limit:**
- Nickname: 1000/day ✅
- Role assignment: None (one-time setup)

**Visibility:**
- ✅ **ALWAYS VISIBLE** in member list
- ✅ Dedicated section at top
- ✅ Clean and professional

**Implementation:**
```rust
// One-time setup:
// 1. Create hoisted role "Queue Status"
// 2. Assign to bot
// 3. Position at top of role hierarchy

// Dynamic updates:
// Just update bot nickname (same as option 2)
bot_member.edit(&ctx.http, EditMember::new().nickname(
    format!("[{}/{}] Ready to play", count, quota)
)).await?;
```

**Pros:**
- ✅ Extremely visible and clean
- ✅ Dedicated section in member list
- ✅ 1000/day nickname updates
- ✅ Professional appearance
- ✅ Can customize role color for visibility
- ✅ Works with multiple groups (different colors/positions)

**Cons:**
- ❌ Requires one-time setup per server
- ❌ Takes up role slot (Discord limit: 250 roles)

**Verdict:** 9.5/10 - **BEST overall solution**

---

### 6. ✅ **Just Dashboard** (CURRENT)

**How it looks:**
- Dashboard embed shows queue count
- Updated via batching system (200ms window)

**Rate Limit:**
- Message edit: 5 per 5 seconds ✅
- Already implemented and working

**Visibility:**
- ❌ **NOT always visible** - requires opening dashboard channel
- ❌ Users must navigate to specific channel

**Pros:**
- ✅ Already implemented
- ✅ Rich formatting (embeds, buttons)
- ✅ Shows detailed info (player names, ranks, etc.)
- ✅ Batching prevents spam

**Cons:**
- OFFt always visible
- ❌ Requires navigation to channel

**Verdict:** 7/10 - Good for details, bad for at-a-glance status

---

## Final Recommendations

### **Option A: Bot Nickname Only** (Simple, Single-Group Servers)
```
Bot appears as: "PUG Bot [Queue: 5/8]"
```
- **Best for:** Servers with 1 group
- **Rate limit:** 1000/day = ~1 update per 86 seconds
- **Visibility:** ⭐⭐⭐⭐⭐
- **Complexity:** (very simple)

---

### **Option B: Hoisted Role + Bot Nickname** (Professional, Multi-Group)
```
Member List:
├── Queue Status [Red]
│   └── PUG Bot [8v8: 5/8]
├── Queue Status [Blue]  
│   └── PUG Bot [4v4: 2/4]
└── Members (10)
```
- **Best for:** Servers with multiple groups
- **Rate limit:** 1000/day per nickname
- **Visibility:** ⭐⭐⭐⭐⭐
- **Complexity:** ⭐(setup roles once, update nicknames)

---

### **Option C: Bot Nickname + Channel Name** (Redundant, Safe)
```
Bot: "PUG Bot [5/8]" (updated frequently)
Channel: "Queue 5/8" (updated only when count changes)
```
- **Best for:** Maximum visibility
- **Rate limit:** 1000/day (bot) + 2/10min (channel, already fixed)
- **Visibility:** ⭐⭐⭐⭐⭐
- **Complexity:** ⭐(two systems)

---

### **Option D: Dashboard Only** (Current, Minimal)
```
Keep current implementation, no changes
```
- **Best for:** Minimal API usage, detailed info only
- **Rate limit:** Already optimized
- **Visibility:** ⭐(requires channel navigation)
- **Complexity:** (already done)

---

## Technical Details

### Bot Nickname API Call
```rust
// Serenity implementation
use serenity::all::{EditMember, GuildId, UserId};

pub async fn update_bot_nickname(
    ctx: &Context,
    guild_id: GuildId,
    count: usize,
    quota: u8,
    group_name: Option<&str>,
) -> Result<()> {
    let bot_id = ctx.cache.current_user().id;
    
    let nickname = if let Some(name) = group_name {
        format!("PUG Bot [{}: {}/{}]", name, count, quota)
    } else {
        format!("PUG Bot [Queue: {}/{}]", count, quota)
    };
    
    // Edit bot's member object
    guild_id.edit_member(
        &ctx.http,
        bot_id,
        EditMember::new().nickname(&nickname)
    ).await?;
    
    Ok(())
}
```

### Channel Topic (Alternative)
```rust
// Can be combined with channel name for dual updates
pub async fn update_vc_topic(
    ctx: &Context,
    channel_id: ChannelId,
    count: usize,
    quota: u8,
) -> Result<()> {
    use serenity::all::EditChannel;
    
    let topic = format!(
        "Queue Status: {}/{} players ready | Join below!",
        count, quota
    );
    
    ctx.http.edit_channel(
        channel_id,
        &EditChannel::new().topic(&topic),
        Some("Update queue count")
    ).await?;
    
    Ok(())
}
```

### Rate Limit Comparison Table

| Method | Rate Limit | Updates/Hour | Always Visible? | Multi-Server Safe? |
|--------|-----------|--------------|-----------------|-------------------|
| Channel Name | 2/10min | 12 | OFF (must see channel) | ON (per-channel) |
| Channel Topic | 2/10min | 12 | OFF (must hover) | ON (per-channel) |
| Category Name | 2/10min | 12 | ON | ON (per-channel) |
| Bot Nickname | 1000/day | ~41 | ON | ON (per-guild) |
| Role Name | 1000/day | ~41 | OFF (not prominent) | ON (per-role) |
| Dashboard | 5/5sec | 3600 | OFF (must open) | ON (per-channel) |

---

## Recommendation

**Use Bot Nickname** (Option A or B depending on multi-group needs):

1. **Always visible** in member list
2. **1000 updates/day** = ~41/hour = plenty for queue activity
3. **Per-guild** rate limit = multi-server safe
4. **Simple implementation**
5. **Professional appearance**
6. **Zero channel clutter**

For servers with multiple groups, use **Option B** (hoisted roles + nicknames) to show multiple queue statuses simultaneously.

Keep dashboard for detailed info (player list, ranks, etc.) and bot nickname for at-a-glance status.
