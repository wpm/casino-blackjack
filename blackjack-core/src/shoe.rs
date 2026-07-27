//! A multi-deck dealing shoe with a cut card and an honest discard pile.
//!
//! The shoe is generic over any [`RngCore`], so tests and replays inject a
//! seeded generator (see [`Shoe::from_seed`]) and get identical shuffles
//! every time.

use rand::seq::SliceRandom;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::card::{Card, Rank, Suit};

/// Number of cards in a single standard deck.
pub const DECK_SIZE: usize = 52;

/// A dealing shoe: several shuffled decks, a cut card, and a discard pile.
///
/// Life cycle per round: [`draw`](Shoe::draw) cards during play, return every
/// card that left the shoe with [`discard`](Shoe::discard) (or burn cards
/// directly with [`burn`](Shoe::burn)), and between rounds check
/// [`cut_card_reached`](Shoe::cut_card_reached) to decide whether to
/// [`reshuffle`](Shoe::reshuffle). Drawing past the cut card keeps working —
/// the cut card only signals that the *next* round should start from a fresh
/// shuffle.
///
/// The discard pile is real, not simulated: every card a player has seen sits
/// either in a hand or in the pile, and [`reshuffle`](Shoe::reshuffle) only
/// recombines cards the shoe actually holds. That makes card counting against
/// the app honest.
#[derive(Debug, Clone)]
pub struct Shoe<R: RngCore> {
    /// Undealt cards; the next card to deal is the last element.
    cards: Vec<Card>,
    /// Cards returned to the shoe after being dealt (or burned).
    discards: Vec<Card>,
    /// Total cards the shoe was built with (`decks * DECK_SIZE`).
    capacity: usize,
    /// Number of cards dealt before the cut card is reached.
    cut_index: usize,
    /// Fraction of the shoe dealt before the cut card, kept for reshuffles.
    penetration: f64,
    rng: R,
}

impl<R: RngCore> Shoe<R> {
    /// Build a shuffled shoe of `decks` standard decks with the cut card at
    /// `penetration` (the fraction of the shoe dealt before reshuffle; a
    /// typical casino value is 0.75).
    ///
    /// # Panics
    ///
    /// Panics if `decks` is zero or `penetration` is not in `(0, 1]`.
    pub fn new(decks: usize, penetration: f64, rng: R) -> Shoe<R> {
        assert!(decks > 0, "a shoe needs at least one deck");
        assert!(
            penetration > 0.0 && penetration <= 1.0,
            "penetration must be in (0, 1], got {penetration}"
        );
        let capacity = decks * DECK_SIZE;
        let mut cards = Vec::with_capacity(capacity);
        for _ in 0..decks {
            for suit in Suit::ALL {
                for rank in Rank::ALL {
                    cards.push(Card::new(rank, suit));
                }
            }
        }
        let mut shoe = Shoe {
            cards,
            discards: Vec::with_capacity(capacity),
            capacity,
            cut_index: cut_index(capacity, penetration),
            penetration,
            rng,
        };
        shoe.cards.shuffle(&mut shoe.rng);
        shoe
    }

    /// Deal the next card, or `None` if the shoe is empty.
    ///
    /// Dealing continues past the cut card; the cut card only flags that a
    /// reshuffle is due between rounds.
    pub fn draw(&mut self) -> Option<Card> {
        self.cards.pop()
    }

    /// Draw the next card and place it straight into the discard pile, as a
    /// dealer burns a card after shuffling. Returns the burned card, or
    /// `None` if the shoe is empty.
    pub fn burn(&mut self) -> Option<Card> {
        let card = self.cards.pop()?;
        self.discards.push(card);
        Some(card)
    }

    /// Return a dealt card to the discard pile at the end of a round.
    pub fn discard(&mut self, card: Card) {
        self.discards.push(card);
    }

    /// Return several dealt cards to the discard pile at once.
    pub fn discard_all<I: IntoIterator<Item = Card>>(&mut self, cards: I) {
        self.discards.extend(cards);
    }

    /// Recombine the discard pile with the undealt cards and shuffle.
    ///
    /// Call this between rounds once [`cut_card_reached`](Shoe::cut_card_reached)
    /// reports true, after every dealt card has been returned via
    /// [`discard`](Shoe::discard). Only cards the shoe holds are shuffled:
    /// cards still in hands stay out until they are discarded.
    pub fn reshuffle(&mut self) {
        self.cards.append(&mut self.discards);
        // Place the cut card a penetration fraction into the freshly
        // shuffled stack. Cards still in hands (dealt but never discarded)
        // stay counted as dealt and do not move the cut card.
        self.cut_index = self.cards_dealt() + cut_index(self.cards.len(), self.penetration);
        self.cards.shuffle(&mut self.rng);
    }

    /// True once the number of dealt cards has reached the cut card.
    pub fn cut_card_reached(&self) -> bool {
        self.cards_dealt() >= self.cut_index
    }

    /// Number of cards dealt (drawn or burned) since the last shuffle.
    pub fn cards_dealt(&self) -> usize {
        self.capacity - self.cards.len()
    }

    /// Number of undealt cards left in the shoe.
    pub fn cards_remaining(&self) -> usize {
        self.cards.len()
    }

    /// Number of cards in the discard pile.
    pub fn discard_pile_size(&self) -> usize {
        self.discards.len()
    }

    /// The cards in the discard pile, oldest first. These are exactly the
    /// cards a counter has seen leave play, which is what keeps counting
    /// against the app honest.
    pub fn discards(&self) -> &[Card] {
        &self.discards
    }

    /// Total cards the shoe was built with.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The cut-card penetration this shoe was configured with.
    pub fn penetration(&self) -> f64 {
        self.penetration
    }
}

impl Shoe<ChaCha8Rng> {
    /// Build a shoe whose shuffles are driven by a ChaCha8 generator seeded
    /// from `seed`. The same seed always produces the same card sequence,
    /// which makes tests and replays deterministic.
    pub fn from_seed(decks: usize, penetration: f64, seed: u64) -> Shoe<ChaCha8Rng> {
        Shoe::new(decks, penetration, ChaCha8Rng::seed_from_u64(seed))
    }
}

/// Number of cards dealt from a shoe of `capacity` cards before a cut card
/// at fraction `penetration` is reached (at least one card, so a cut card is
/// always reachable).
fn cut_index(capacity: usize, penetration: f64) -> usize {
    (((capacity as f64) * penetration).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn drain<R: RngCore>(shoe: &mut Shoe<R>) -> Vec<Card> {
        std::iter::from_fn(|| shoe.draw()).collect()
    }

    #[test]
    fn shoe_holds_deck_count_times_fifty_two_cards() {
        for decks in [1, 2, 6, 8] {
            let shoe = Shoe::from_seed(decks, 0.75, 0);
            assert_eq!(shoe.capacity(), decks * DECK_SIZE);
            assert_eq!(shoe.cards_remaining(), decks * DECK_SIZE);
            assert_eq!(shoe.cards_dealt(), 0);
            assert_eq!(shoe.discard_pile_size(), 0);
        }
    }

    #[test]
    fn shoe_contains_every_card_the_right_number_of_times() {
        let decks = 6;
        let mut shoe = Shoe::from_seed(decks, 0.75, 42);
        let mut counts: HashMap<Card, usize> = HashMap::new();
        for card in drain(&mut shoe) {
            *counts.entry(card).or_default() += 1;
        }
        assert_eq!(counts.len(), DECK_SIZE);
        assert!(counts.values().all(|&n| n == decks));
    }

    #[test]
    fn drawing_past_the_last_card_returns_none() {
        let mut shoe = Shoe::from_seed(1, 1.0, 7);
        for _ in 0..DECK_SIZE {
            assert!(shoe.draw().is_some());
        }
        assert_eq!(shoe.cards_remaining(), 0);
        assert_eq!(shoe.cards_dealt(), DECK_SIZE);
        assert_eq!(shoe.draw(), None);
        assert_eq!(shoe.burn(), None);
    }

    #[test]
    fn same_seed_yields_the_same_sequence() {
        let mut a = Shoe::from_seed(6, 0.75, 1234);
        let mut b = Shoe::from_seed(6, 0.75, 1234);
        assert_eq!(drain(&mut a), drain(&mut b));
    }

    #[test]
    fn different_seeds_yield_different_sequences() {
        let mut a = Shoe::from_seed(6, 0.75, 1);
        let mut b = Shoe::from_seed(6, 0.75, 2);
        assert_ne!(drain(&mut a), drain(&mut b));
    }

    #[test]
    fn cut_card_is_reached_at_the_configured_penetration() {
        let mut shoe = Shoe::from_seed(1, 0.5, 9);
        // Cut card sits after floor(52 * 0.5) = 26 cards.
        for _ in 0..25 {
            shoe.draw();
            assert!(!shoe.cut_card_reached());
        }
        shoe.draw();
        assert!(shoe.cut_card_reached());
    }

    #[test]
    fn drawing_past_the_cut_card_still_works() {
        let mut shoe = Shoe::from_seed(1, 0.25, 3);
        while !shoe.cut_card_reached() {
            shoe.draw();
        }
        let dealt_at_cut = shoe.cards_dealt();
        assert_eq!(dealt_at_cut, DECK_SIZE / 4);
        // The round in progress can keep drawing to the last card.
        let rest = drain(&mut shoe);
        assert_eq!(rest.len(), DECK_SIZE - dealt_at_cut);
        assert_eq!(shoe.cards_remaining(), 0);
        assert!(shoe.cut_card_reached());
    }

    #[test]
    fn full_penetration_reaches_the_cut_card_only_on_the_last_card() {
        let mut shoe = Shoe::from_seed(1, 1.0, 11);
        for _ in 0..(DECK_SIZE - 1) {
            shoe.draw();
            assert!(!shoe.cut_card_reached());
        }
        shoe.draw();
        assert!(shoe.cut_card_reached());
    }

    #[test]
    fn burning_and_discarding_feed_the_discard_pile() {
        let mut shoe = Shoe::from_seed(2, 0.75, 5);
        let burned = shoe.burn().unwrap();
        assert_eq!(shoe.discard_pile_size(), 1);
        assert_eq!(shoe.discards(), [burned]);
        assert_eq!(shoe.cards_dealt(), 1);

        let dealt: Vec<Card> = (0..4).map(|_| shoe.draw().unwrap()).collect();
        assert_eq!(shoe.cards_dealt(), 5);
        assert_eq!(shoe.discard_pile_size(), 1);

        shoe.discard_all(dealt.clone());
        assert_eq!(shoe.discard_pile_size(), 5);
        assert_eq!(&shoe.discards()[1..], dealt);

        // Every card is accounted for: in the shoe or in the pile.
        assert_eq!(
            shoe.cards_remaining() + shoe.discard_pile_size(),
            shoe.capacity()
        );
    }

    #[test]
    fn reshuffle_restores_a_fully_discarded_shoe() {
        let mut shoe = Shoe::from_seed(1, 0.5, 21);
        while let Some(card) = shoe.draw() {
            shoe.discard(card);
        }
        assert!(shoe.cut_card_reached());
        assert_eq!(shoe.discard_pile_size(), DECK_SIZE);

        shoe.reshuffle();
        assert_eq!(shoe.cards_remaining(), DECK_SIZE);
        assert_eq!(shoe.cards_dealt(), 0);
        assert_eq!(shoe.discard_pile_size(), 0);
        assert!(!shoe.cut_card_reached());
    }

    #[test]
    fn reshuffle_shuffles_only_cards_the_shoe_holds() {
        let mut shoe = Shoe::from_seed(1, 0.5, 8);
        // Draw three cards and keep them "in hand" — do not discard.
        let in_hand: Vec<Card> = (0..3).map(|_| shoe.draw().unwrap()).collect();
        // Discard two more.
        for _ in 0..2 {
            let card = shoe.draw().unwrap();
            shoe.discard(card);
        }
        shoe.reshuffle();
        // The three in-hand cards stay out of the shoe.
        assert_eq!(shoe.cards_remaining(), DECK_SIZE - 3);
        assert_eq!(shoe.discard_pile_size(), 0);
        let mut all = drain(&mut shoe);
        all.extend(in_hand);
        all.sort();
        let mut full: Vec<Card> = Shoe::from_seed(1, 0.5, 0).cards;
        full.sort();
        assert_eq!(all, full);
    }

    #[test]
    fn reshuffle_produces_a_different_order_than_dealt() {
        let mut shoe = Shoe::from_seed(6, 0.75, 99);
        let before = drain(&mut shoe);
        shoe.discard_all(before.iter().copied());
        shoe.reshuffle();
        let after = drain(&mut shoe);
        assert_eq!(before.len(), after.len());
        // With 312 cards the odds of an identical order are astronomically
        // small; a fixed seed makes this check fully deterministic anyway.
        assert_ne!(before, after);
    }

    #[test]
    fn generic_seam_accepts_any_rng_core() {
        // Any RngCore works; use the ChaCha generator through the generic
        // constructor to prove the seam.
        let rng = ChaCha8Rng::seed_from_u64(77);
        let mut a = Shoe::new(2, 0.6, rng);
        let mut b = Shoe::from_seed(2, 0.6, 77);
        assert_eq!(drain(&mut a), drain(&mut b));
    }

    #[test]
    #[should_panic(expected = "at least one deck")]
    fn zero_decks_panics() {
        let _ = Shoe::from_seed(0, 0.75, 0);
    }

    #[test]
    #[should_panic(expected = "penetration")]
    fn zero_penetration_panics() {
        let _ = Shoe::from_seed(1, 0.0, 0);
    }

    #[test]
    #[should_panic(expected = "penetration")]
    fn over_unity_penetration_panics() {
        let _ = Shoe::from_seed(1, 1.5, 0);
    }
}
