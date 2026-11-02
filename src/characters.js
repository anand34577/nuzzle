// Built-in characters. All of them are original designs and share one rig (the same part ids), so a single
// animation engine drives every one, while each keeps its own temperament through `traits`.
//
// Required ids : shadow, root, feet(footL/footR), body, armL/armR, face, eyeL/eyeR, pupL/pupR, hiL/hiR,
//                happyL/happyR, smile, mouth, zzz(z1-z3)
// Optional ids : antenna(+bulb), antenna2, earL/earR, tail, puff, flame, moss, drop, cheekL/cheekR
// Optional parts carry data-pivot="x y": the point they rotate around.
//
// traits (all multipliers, 1 = Pip):
//   bounce  how springy walking is        speed   walk-cycle tempo         sway   body tilt amount
//   energy  arm / gesture size            blink   blink frequency          lid    resting eyelid openness (1 = wide)
//   waddle  extra side-to-side tilt       float   hovers instead of stepping
//   quirks  idle behaviours it may do     pokes   things it says when poked

const CHARACTERS = [
  {
    id: 'pip', name: 'Pip', tagline: 'Curious sprout',
    skin: '#ffc9a3', belly: '#ffe3cf', limb: '#f4b08a', foot: '#e9a07a', cheek: '#ff9aa2', eye: '#3a2d46', line: '#6b3f4a', mouth: '#8a3c4c',
    antenna: { stem: '#7bbf8e', bulb: '#9be3b0' },
    traits: { bounce: 1, speed: 1, sway: 1, energy: 1, blink: 1, quirks: ['lookaround', 'hum', 'antenna', 'stretch'] },
    pokes: ['Hehe!', 'That tickles!', 'Boop!'], greet: 'Hi! I am Pip.',
  },
  {
    id: 'mochi', name: 'Mochi', tagline: 'Cosy cat',
    skin: '#fff3e6', belly: '#ffffff', limb: '#f6dfc8', foot: '#f0cfae', cheek: '#ffa9b8', eye: '#3b2f3f', line: '#7a4a55', mouth: '#9a4a5a',
    behind: `
      <g id="tail" data-pivot="152 206"><path d="M150 208 Q212 206 198 146" stroke="#f6dfc8" stroke-width="13" fill="none" stroke-linecap="round"/></g>
      <g id="earL" data-pivot="76 112"><path d="M50 124 L52 58 L98 100 Z" fill="#fff3e6"/><path d="M58 106 L60 76 L84 98 Z" fill="#ffc2cf"/></g>
      <g id="earR" data-pivot="124 112"><path d="M150 124 L148 58 L102 100 Z" fill="#fff3e6"/><path d="M142 106 L140 76 L116 98 Z" fill="#ffc2cf"/></g>`,
    faceExtra: `
      <ellipse cx="100" cy="157" rx="4.5" ry="3.2" fill="#ff9fb2"/>
      <path d="M40 158 L66 162 M40 170 L66 168 M160 158 L134 162 M160 170 L134 168" stroke="#7a4a55" stroke-width="2" stroke-linecap="round" opacity=".55"/>`,
    traits: { bounce: 0.8, speed: 0.95, sway: 1.2, energy: 0.8, blink: 1, quirks: ['tailWag', 'earTwitch', 'stretch', 'lookaround', 'yawn'] },
    pokes: ['Mrrp!', 'Hey, mind the fur!', 'Purrr...'], greet: 'Mrrp. Hello.',
  },
  {
    id: 'bruno', name: 'Bruno', tagline: 'Cuddly bear',
    skin: '#c98f5f', belly: '#f1d3b0', limb: '#b87d4d', foot: '#a86f42', cheek: '#ff9a8a', eye: '#2d2118', line: '#4a2d1e', mouth: '#7a3430',
    behind: `
      <g id="earL" data-pivot="52 112"><circle cx="52" cy="104" r="19" fill="#c98f5f"/><circle cx="52" cy="104" r="10" fill="#f1d3b0"/></g>
      <g id="earR" data-pivot="148 112"><circle cx="148" cy="104" r="19" fill="#c98f5f"/><circle cx="148" cy="104" r="10" fill="#f1d3b0"/></g>`,
    front: '<ellipse cx="100" cy="170" rx="30" ry="24" fill="#f1d3b0"/>',
    faceExtra: '<ellipse cx="100" cy="159" rx="8" ry="5.5" fill="#3b2418"/>',
    traits: { bounce: 0.7, speed: 0.85, sway: 0.8, energy: 0.7, blink: 0.8, quirks: ['hum', 'stretch', 'yawn', 'lookaround'] },
    pokes: ['Oof, gently!', 'Hug time?', 'Hehe, tummy!'], greet: 'Well hello, friend.',
  },
  {
    id: 'zap', name: 'Zap', tagline: 'Helper robot',
    skin: '#bfe3ff', belly: '#e8f5ff', limb: '#9fcdf2', foot: '#7fb4e0', cheek: '#7cc4ff', eye: '#16324f', line: '#2f5d8a', mouth: '#2f5d8a',
    antenna: { stem: '#8aa6bf', bulb: '#ffe27a' },
    behind: '<rect x="30" y="148" width="14" height="30" rx="7" fill="#9fcdf2"/><rect x="156" y="148" width="14" height="30" rx="7" fill="#9fcdf2"/>',
    front: `
      <rect x="80" y="196" width="40" height="16" rx="8" fill="#9fcdf2"/>
      <circle cx="91" cy="204" r="3" fill="#ffe27a"/><circle cx="102" cy="204" r="3" fill="#7cd6a0"/><circle cx="113" cy="204" r="3" fill="#ff8aa0"/>`,
    traits: { bounce: 1.2, speed: 1.15, sway: 0.6, energy: 1.3, blink: 0.9, quirks: ['antenna', 'wiggle', 'lookaround', 'hop'] },
    pokes: ['Bzzt!', 'Ping received.', 'Ooh, sparks!'], greet: 'Systems online. Hello!',
  },
  {
    id: 'nimbus', name: 'Nimbus', tagline: 'Dreamy cloud',
    skin: '#e6f0fb', belly: '#ffffff', limb: '#d3e3f6', foot: '#c3d7ee', cheek: '#ffb5c6', eye: '#35476b', line: '#5a6f9a', mouth: '#7a5a8a',
    body: { rx: 68, ry: 56, cy: 164 },
    behind: `
      <g id="puff" data-pivot="100 120" fill="#e6f0fb"><circle cx="58" cy="124" r="26"/><circle cx="100" cy="104" r="32"/><circle cx="142" cy="124" r="26"/></g>
      <ellipse id="drop" cx="150" cy="150" rx="4" ry="6" fill="#8fc0f0" opacity="0"/>`,
    traits: { bounce: 0.6, speed: 0.8, sway: 1.4, energy: 0.8, blink: 0.9, float: true, quirks: ['drip', 'hum', 'lookaround', 'yawn'] },
    pokes: ['Poof!', 'Eep, I nearly rained!', 'Fluffy alert!'], greet: 'Oh... hi there.',
  },
  {
    id: 'pebble', name: 'Pebble', tagline: 'Calm little stone',
    skin: '#9ea9b1', belly: '#bcc5cc', limb: '#8c979f', foot: '#7e8a92', cheek: '#c7a6a0', eye: '#2a3038', line: '#4b555d', mouth: '#4b555d',
    body: { rx: 70, ry: 58, cy: 164 },
    front: '<circle cx="64" cy="140" r="3" fill="#8c979f"/><circle cx="150" cy="170" r="4" fill="#8c979f"/><circle cx="132" cy="128" r="2.5" fill="#8c979f"/><circle cx="72" cy="188" r="3.5" fill="#8c979f"/>',
    behind: `<g id="moss" data-pivot="96 108"><ellipse cx="94" cy="108" rx="22" ry="8" fill="#7fbf8e"/><path d="M90 104 Q84 88 96 82 Q98 94 92 104Z" fill="#6aae7c"/><path d="M100 104 Q108 92 118 96 Q112 104 102 106Z" fill="#8ccf9a"/></g>`,
    traits: { bounce: 0.45, speed: 0.55, sway: 0.5, energy: 0.5, blink: 0.55, lid: 0.78, quirks: ['stretch', 'yawn', 'lookaround'] },
    pokes: ['...hm?', 'I was thinking.', 'Steady as a rock.'], greet: 'Oh. Hello. Take your time.',
  },
  {
    id: 'ember', name: 'Ember', tagline: 'Clever fox',
    skin: '#f4a259', belly: '#fff1dc', limb: '#dd8a3c', foot: '#7a4a2a', cheek: '#fff1dc', eye: '#3a2218', line: '#6b3a1e', mouth: '#8a3c2c',
    behind: `
      <g id="tail" data-pivot="150 200"><path d="M148 214 Q226 210 206 132 Q196 166 160 176 Z" fill="#f4a259"/><path d="M206 132 Q214 156 204 176 Q196 166 198 150 Z" fill="#fff1dc"/></g>
      <g id="earL" data-pivot="74 112"><path d="M48 120 L56 52 L100 98 Z" fill="#f4a259"/><path d="M56 52 L64 78 L48 82 Z" fill="#5a3320"/></g>
      <g id="earR" data-pivot="126 112"><path d="M152 120 L144 52 L100 98 Z" fill="#f4a259"/><path d="M144 52 L136 78 L152 82 Z" fill="#5a3320"/></g>`,
    front: '<path d="M62 170 Q100 150 138 170 Q120 206 100 208 Q80 206 62 170Z" fill="#fff1dc"/>',
    faceExtra: '<ellipse cx="100" cy="158" rx="5" ry="3.6" fill="#3a2218"/>',
    traits: { bounce: 1.15, speed: 1.2, sway: 1.1, energy: 1.2, blink: 1, quirks: ['tailWag', 'earTwitch', 'hop', 'lookaround'] },
    pokes: ['Hah, gotcha!', 'Tail is off limits!', 'Bold move, friend.'], greet: 'Well, look who is here.',
  },
  {
    id: 'bun', name: 'Bun', tagline: 'Shy bunny',
    skin: '#f3eefb', belly: '#ffffff', limb: '#e3d9f2', foot: '#d6c9ea', cheek: '#ffb3c8', eye: '#3b2f4a', line: '#7a5a8a', mouth: '#a0587a',
    behind: `
      <circle cx="152" cy="204" r="12" fill="#ffffff"/>
      <g id="earL" data-pivot="84 104"><rect x="66" y="14" width="28" height="92" rx="14" fill="#f3eefb"/><rect x="74" y="28" width="12" height="64" rx="6" fill="#ffc6d6"/></g>
      <g id="earR" data-pivot="116 104"><rect x="106" y="14" width="28" height="92" rx="14" fill="#f3eefb"/><rect x="114" y="28" width="12" height="64" rx="6" fill="#ffc6d6"/></g>`,
    faceExtra: '<ellipse cx="100" cy="157" rx="4" ry="3" fill="#ff9fb8"/>',
    traits: { bounce: 1.3, speed: 1.05, sway: 0.9, energy: 0.9, blink: 1.3, quirks: ['earTwitch', 'hop', 'lookaround', 'wiggle'] },
    pokes: ['Eek! ...hi.', 'Oh! You startled me.', 'Binky!'], greet: 'H-hello... it is nice to see you.',
  },
  {
    id: 'axo', name: 'Axo', tagline: 'Goofy axolotl',
    skin: '#ffb9c8', belly: '#ffe3ea', limb: '#f59db3', foot: '#ee8ea6', cheek: '#ff7fa0', eye: '#3a2a3a', line: '#8a4560', mouth: '#a4485f',
    behind: `
      <g id="tail" data-pivot="148 206"><path d="M148 206 Q196 214 204 176 Q190 190 156 190 Z" fill="#ffb9c8" opacity=".9"/></g>
      <g id="earL" data-pivot="56 124" stroke="#ff8fab" stroke-width="6" stroke-linecap="round" fill="none"><path d="M54 118 Q28 100 22 84"/><path d="M52 130 Q22 124 14 112"/><path d="M56 142 Q28 148 18 140"/></g>
      <g id="earR" data-pivot="144 124" stroke="#ff8fab" stroke-width="6" stroke-linecap="round" fill="none"><path d="M146 118 Q172 100 178 84"/><path d="M148 130 Q178 124 186 112"/><path d="M144 142 Q172 148 182 140"/></g>`,
    traits: { bounce: 1.1, speed: 0.9, sway: 1.3, energy: 1.2, blink: 1, quirks: ['wiggle', 'earTwitch', 'hum', 'tailWag', 'lookaround'] },
    pokes: ['Blub!', 'Wiggle wiggle!', 'Hehe, splash!'], greet: 'Hiii! You are here!',
  },
  {
    id: 'wisp', name: 'Wisp', tagline: 'Gentle spirit',
    skin: '#dcf6f0', belly: '#f4fffc', limb: '#c2ebe3', foot: '#b2e1d8', cheek: '#9fe8d2', eye: '#1f4a47', line: '#3f8077', mouth: '#3f8077',
    body: { rx: 56, ry: 66, cy: 156 },
    behind: `<g id="flame" data-pivot="100 96"><path d="M100 98 Q80 70 100 30 Q120 70 100 98Z" fill="#8fe6cf"/><path d="M100 94 Q90 74 100 52 Q110 74 100 94Z" fill="#d8fff4"/></g>`,
    traits: { bounce: 0.55, speed: 0.7, sway: 1.5, energy: 0.7, blink: 0.8, float: true, quirks: ['flicker', 'hum', 'lookaround', 'yawn'] },
    pokes: ['Oh... hello.', 'Whoosh!', 'Careful, I am warm.'], greet: 'A quiet hello to you.',
  },
  {
    id: 'penny', name: 'Penny', tagline: 'Tidy penguin',
    skin: '#3d4b60', belly: '#f7f9fb', limb: '#2f3b4c', foot: '#f5a742', cheek: '#ff9aa8', eye: '#10151c', line: 'none', mouth: '#e0892c',
    body: { rx: 58, ry: 68, cy: 154 },
    front: '<ellipse cx="100" cy="176" rx="40" ry="42" fill="#f7f9fb"/>',
    faceExtra: '<path d="M91 157 L109 157 L100 171 Z" fill="#f5a742"/>',
    traits: { bounce: 0.9, speed: 0.85, sway: 1, energy: 0.9, blink: 1, waddle: 1.8, quirks: ['stretch', 'hum', 'lookaround', 'wiggle'] },
    pokes: ['Excuse me!', 'Professional penguin here.', 'Waddle on.'], greet: 'Good day! Shall we make a plan?',
  },
  {
    id: 'cosmo', name: 'Cosmo', tagline: 'Curious alien',
    skin: '#bcaaf2', belly: '#e7deff', limb: '#a893ea', foot: '#927dde', cheek: '#ff9fd2', eye: '#241a4a', line: '#5a3f9a', mouth: '#5a3f9a',
    antenna: { stem: '#8f7bd9', bulb: '#8ff0d4' },
    behind: `<g id="antenna2" data-pivot="100 92"><path d="M104 92 Q122 66 138 58" stroke="#8f7bd9" stroke-width="5" fill="none" stroke-linecap="round"/><circle cx="140" cy="56" r="7" fill="#ffd1f0"/></g>`,
    faceExtra: '<circle cx="82" cy="170" r="1.8" fill="#927dde"/><circle cx="92" cy="176" r="1.8" fill="#927dde"/><circle cx="118" cy="176" r="1.8" fill="#927dde"/>',
    traits: { bounce: 1.1, speed: 1, sway: 1, energy: 1.1, blink: 1.1, quirks: ['antenna', 'wiggle', 'lookaround', 'hop', 'stretch'] },
    pokes: ['Greetings, Earthling!', 'Zorp!', 'Ooh, a poke!'], greet: 'Greetings! What a lovely planet.',
  },
];

const DEFAULT_CHARACTER = 'pip';

// What each one says on its own now and then (chatter), when petted, and where poking gets a special answer.
const LINES = {
  pip: { chatter: ['I wonder what clouds taste like.', 'Fun fact: I am about 40% curiosity.', 'Need a hand? Just ask.', 'Your screen looks busy today.'],
    petted: ['Hehe, that is nice.', 'More head pats, please!'] },
  mochi: { chatter: ['*stretches* Is it nap time yet?', 'I knocked nothing off your desk today. You are welcome.', 'Mrrp. Just checking on you.'],
    petted: ['Purrrrr...', 'Mm, right behind the ears.'], zones: { feet: ['Paws off the paws!'], head: ['Mrrrp, yes, there.'] } },
  bruno: { chatter: ['Some honey would be nice right now.', 'Remember to drink some water, friend.', 'Bear hugs available on request.'],
    petted: ['Ahh, cosy.', 'Big warm thanks.'] },
  zap: { chatter: ['Running diagnostics... you look great.', 'Beep. Hydration reminder.', 'My circuits enjoy your company.'],
    petted: ['Static levels: delightful.', 'Bzzz... recharging.'], zones: { head: ['Antenna realigned. Thank you.'], nose: ['Button pressed. Nothing happened.'] } },
  nimbus: { chatter: ['Drifting along...', 'I might drizzle a little. Just kidding.', 'The sky in here is very calm.'],
    petted: ['So floaty...', 'Mmm, soft.'] },
  pebble: { chatter: ['...', 'Still here. Being a rock.', 'Slow is fine.'],
    petted: ['Smooth.', '...nice.'], zones: { head: ['Careful, that is my moss.'] } },
  ember: { chatter: ['I had a clever idea. I forgot it.', 'Need a shortcut? I know a few.', 'Quick as a fox, quiet as one too.'],
    petted: ['Okay, that is nice.', 'Do not tell anyone I liked that.'] },
  bun: { chatter: ['Um... you are doing great.', '*nibbles a carrot quietly*', 'I like it here.'],
    petted: ['Oh! ...okay, that is lovely.', '*happy thump*'], zones: { head: ['My ears! ...it is okay, gently.'] } },
  axo: { chatter: ['Blub. Just saying hi.', 'Did you know I can smile forever?', 'Wiggle break? Wiggle break.'],
    petted: ['Hehe, squishy!', 'Wheee, pats!'] },
  wisp: { chatter: ['A small light, keeping you company.', 'Breathe in... and out.', 'Everything is a little softer at night.'],
    petted: ['Warm... thank you.', 'I glow a bit brighter.'], zones: { head: ['Mind the flame!'] } },
  penny: { chatter: ['Shall we tidy up a little?', 'Agenda item one: a snack.', 'Waddling with purpose.'],
    petted: ['Very kind. Noted.', 'Feathers: properly smoothed.'], zones: { nose: ['That is a beak, thank you.'] } },
  cosmo: { chatter: ['Your planet has such nice pixels.', 'Report for home: humans are fun.', 'Zorp. That means hello. Probably.'],
    petted: ['Earth custom detected: head pat. Approved!', 'Zorp zorp!'] },
};

function characterMeta(id) {
  const c = CHARACTERS.find((x) => x.id === id) || CHARACTERS[0];
  return { ...c, ...(LINES[c.id] || {}) };
}

function characterSvg(id, { forPreview = false } = {}) {
  const c = characterMeta(id);
  const b = { rx: 62, ry: 64, cy: 158, ...(c.body || {}) };
  const bellyCy = b.cy + 24;
  const antenna = c.antenna ? `
        <g id="antenna" data-pivot="100 92">
          <path d="M100 92 Q96 62 108 48" stroke="${c.antenna.stem}" stroke-width="5" fill="none" stroke-linecap="round"/>
          <circle id="bulb" cx="108" cy="46" r="8" fill="${c.antenna.bulb}"/>
        </g>` : '';
  const eye = (side, cx) => `
          <g id="eye${side}" transform="translate(${cx} 140)"><ellipse rx="14" ry="18" fill="#fff"/><circle id="pup${side}" r="8" fill="${c.eye}"/><circle id="hi${side}" cx="3" cy="-3.5" r="3.4" fill="#fff"/><circle id="lo${side}" cx="-3" cy="3.5" r="1.6" fill="#fff" opacity=".85"/></g>
          <path id="happy${side}" d="M${cx - 12} 142 Q${cx} 126 ${cx + 12} 142" stroke="${c.eye}" stroke-width="4" fill="none" stroke-linecap="round" display="none"/>
          <path id="dizzy${side}" d="M0 0 m-2 0 a2 2 0 1 1 4 0 a4 4 0 1 1 -8 0 a6 6 0 1 1 12 0 a8 8 0 1 1 -16 0" transform="translate(${cx} 140)" stroke="${c.eye}" stroke-width="2.6" fill="none" stroke-linecap="round" display="none"/>
          <path id="brow${side}" d="M${cx - 11} 114 Q${cx} 109 ${cx + 11} 114" stroke="${c.eye}" stroke-width="3.4" fill="none" stroke-linecap="round" display="none"/>`;
  const smileStroke = c.line === 'none' ? 'none' : c.line;
  const svg = `
    <ellipse id="shadow" cx="100" cy="226" rx="52" ry="8" fill="rgba(0,0,0,.2)"/>
    <g id="root">
      <g id="feet">
        <ellipse id="footL" cx="76" cy="220" rx="17" ry="9" fill="${c.foot}"/>
        <ellipse id="footR" cx="124" cy="220" rx="17" ry="9" fill="${c.foot}"/>
      </g>
      <g id="body">
        ${c.behind || ''}${antenna}
        <g id="armL" transform="translate(52 150) rotate(14)"><ellipse cx="0" cy="14" rx="9" ry="16" fill="${c.limb}"/></g>
        <g id="armR" transform="translate(148 150) rotate(-14)"><ellipse cx="0" cy="14" rx="9" ry="16" fill="${c.limb}"/></g>
        <ellipse cx="100" cy="${b.cy}" rx="${b.rx}" ry="${b.ry}" fill="${c.skin}"/>
        <ellipse cx="100" cy="${bellyCy}" rx="38" ry="30" fill="${c.belly}"/>
        ${c.front || ''}
        <g id="face">${eye('L', 74)}${eye('R', 126)}
          <ellipse id="cheekL" cx="62" cy="158" rx="9" ry="5.5" fill="${c.cheek}" opacity=".55"/>
          <ellipse id="cheekR" cx="138" cy="158" rx="9" ry="5.5" fill="${c.cheek}" opacity=".55"/>
          ${c.faceExtra || ''}
          <path id="smile" d="M88 166 Q100 171 112 166" stroke="${smileStroke}" stroke-width="3.2" fill="none" stroke-linecap="round"/>
          <ellipse id="mouth" fill="${c.mouth}" display="none"/>
        </g>
      </g>
    </g>
    <g id="zzz" opacity="0" fill="#8a7f96" font-weight="700" font-family="system-ui, sans-serif">
      <text id="z1" x="150" y="84" font-size="16">z</text>
      <text id="z2" x="162" y="62" font-size="20">z</text>
      <text id="z3" x="176" y="36" font-size="26">Z</text>
    </g>`;
  // Previews appear several times on one page, so they must not carry duplicate ids.
  return forPreview ? svg.replace(/ id="[^"]*"/g, '') : svg;
}
