//! The metadata suites, as one test binary of their own: one link
//! instead of one per file. Each file is its own module, named as it was.

#[macro_use]
mod corpus;
mod support;

mod a_grown_rbm_array_reaches_the_file;
mod a_named_reference_arrives_as_it_came;
mod a_schema_is_data_not_a_build_input;
mod an_rbm_class_with_a_vtable_starts_after_it;
mod binary_detect;
mod binary_xml_crossover;
mod dump_includes;
mod enum_variant_sentinels;
mod from_scratch;
mod instance_roundtrip;
mod meta_census;
mod meta_decode_quality;
mod pso_roundtrip;
mod rbf_roundtrip;
mod rbm_roundtrip;
mod rbm_synthesis_fixed_point;
mod snapshot_load;
mod synthesis_fixed_point;
mod synthesis_places_heap_members;
mod the_psc_simple_flag_matches_the_shipped_vtable_word;
mod typed_creature_rbf;
mod typed_ped_schema;
mod typed_ped_variation;
mod typed_scenario_regions;
mod typed_weapons;
mod typed_world_roots;
mod what_a_float16_stores;
mod what_a_page_flag_can_express;
mod what_converges_from_the_struct_definitions;
mod what_survives_an_xml_escape;
mod what_the_corpus_asks_to_be_named;
mod what_the_psc_says_an_array_is;
mod what_the_xml_cannot_say;
mod when_a_snapshot_is_older_than_its_generator;
mod which_hash_pso_stores;
mod xml_converges_against_the_snapshot;
mod xml_converges_without_a_snapshot;
