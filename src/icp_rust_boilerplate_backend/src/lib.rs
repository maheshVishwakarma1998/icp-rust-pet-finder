#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};
use ic_cdk::caller;

// Memory and ID Counter
type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

// Define the Pet struct with the necessary attributes for each pet.
#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Pet {
    id: u64,
    pet_name: String,
    pet_breed: String,
    pet_color: String,
    pet_photo: String,
    owner: String,
    is_lost: bool,
    lost_location: Option<String>,
    created_at: u64,
    updated_at: Option<u64>,
}


// Struct for FoundPetReport to track information about found pets
#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct FoundPetReport {
    pet_id: u64,
    finder_name: String,
    found_location: String,
    created_at: u64,
}

// Traits for Storable and BoundedStorable
impl Storable for Pet {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }
    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl Storable for FoundPetReport {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }
    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Pet {
    const MAX_SIZE: u32 = 1024;
    const IS_FIXED_SIZE: bool = false;
}

impl BoundedStorable for FoundPetReport {
    const MAX_SIZE: u32 = 512;
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static PET_STORAGE: RefCell<StableBTreeMap<u64, Pet, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));

    static FOUND_PET_STORAGE: RefCell<StableBTreeMap<u64, FoundPetReport, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2)))
    ));
}

// Define payload for registering a pet
#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct PetPayload {
    pet_name: String,
    pet_breed: String,
    pet_color: String,
    pet_photo: String,
}

// Define payload for reporting a found pet
#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct FoundPetReportPayload {
    finder_name: String,
    found_location: String,
}

// Register a new pet to the registry
#[ic_cdk::update]
fn register_pet(payload: PetPayload) -> Option<Pet> {
        // Increment the pet ID counter
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("Cannot increment ID counter");
    
    // Create a new pet with the provided data
    let pet = Pet {
        id,
        pet_name: payload.pet_name,
        pet_breed: payload.pet_breed,
        pet_color: payload.pet_color,
        pet_photo: payload.pet_photo,
        owner: caller().to_string(),
        is_lost: false,
        lost_location: None,
        created_at: time(),
        updated_at: None,
    };
    do_insert_pet(&pet);
    Some(pet)
}


// Report a pet as lost
#[ic_cdk::update]
fn report_lost_pet(id: u64, lost_location: String) -> Result<Pet, Error> {
    match PET_STORAGE.with(|storage| storage.borrow().get(&id)) {
        Some(mut pet) => {
            if pet.owner != caller().to_string() {
                return Err(Error::NotAuthorized {
                    msg: "You are not the owner".to_string(),
                });
            }
            pet.is_lost = true;
            pet.lost_location = Some(lost_location);
            pet.updated_at = Some(time());
            do_insert_pet(&pet);
            Ok(pet)
        }
        None => Err(Error::NotFound {
            msg: format!("Pet with id {} not found", id),
        }),
    }
}

// Report a found pet and update its status
#[ic_cdk::update]
fn report_found_pet(id: u64, payload: FoundPetReportPayload) -> Result<Pet, Error> {
    match PET_STORAGE.with(|storage| storage.borrow().get(&id)) {
        Some(mut pet) => {
            if !pet.is_lost {
                return Err(Error::NotAuthorized {
                    msg: "Pet is not reported as lost".to_string(),
                });
            }
            let report = FoundPetReport {
                pet_id: id,
                finder_name: payload.finder_name,
                found_location: payload.found_location,
                created_at: time(),
            };
            FOUND_PET_STORAGE.with(|storage| storage.borrow_mut().insert(id, report));
            pet.is_lost = false;
            pet.lost_location = None;
            pet.updated_at = Some(time());
            do_insert_pet(&pet);
            Ok(pet)
        }
        None => Err(Error::NotFound {
            msg: format!("Pet with id {} not found", id),
        }),
    }
}


// Delete a pet from the registry
#[ic_cdk::update]
fn delete_pet(id: u64) -> Result<String, Error> {
    PET_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        if let Some(pet) = storage.get(&id) {
            if pet.owner != caller().to_string() {
                return Err(Error::NotAuthorized {
                    msg: "You are not the owner of this pet".to_string(),
                });
            }
            storage.remove(&id);
            Ok(format!("Pet with ID {} has been successfully deleted.", id))
        } else {
            Err(Error::NotFound {
                msg: format!("Pet with ID {} not found", id),
            })
        }
    })
}


// Update a pet by it's ID
#[ic_cdk::update]
fn update_pet_info(id: u64, payload: PetPayload) -> Result<Pet, Error> {
    PET_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        if let Some(mut pet) = storage.get(&id) {
            if pet.owner != caller().to_string() {
                return Err(Error::NotAuthorized {
                    msg: "You are not the owner of this pet".to_string(),
                });
            }
            pet.pet_name = payload.pet_name;
            pet.pet_breed = payload.pet_breed;
            pet.pet_color = payload.pet_color;
            pet.pet_photo = payload.pet_photo;
            pet.updated_at = Some(time());
            storage.insert(id, pet.clone());
            Ok(pet)
        } else {
            Err(Error::NotFound {
                msg: format!("Pet with ID {} not found", id),
            })
        }
    })
}


// Retrieve all registered pets
#[ic_cdk::query]
fn get_all_pets() -> Vec<Pet> {
    PET_STORAGE.with(|storage| storage.borrow().iter().map(|(_, pet)| pet).collect())
}

// Retrieve a specific pet by ID
#[ic_cdk::query]
fn get_pet(id: u64) -> Option<Pet> {
    PET_STORAGE.with(|storage| storage.borrow().get(&id))
}

// Helper function to insert a pet into the storage
fn do_insert_pet(pet: &Pet) {
    PET_STORAGE.with(|storage| storage.borrow_mut().insert(pet.id, pet.clone()));
}


// Define error types for handling errors in the system
#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
    NotAuthorized { msg: String },
}

// Export candid
ic_cdk::export_candid!();

